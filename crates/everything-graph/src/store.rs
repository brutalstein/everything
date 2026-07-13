use crate::extractor::{ExtractedFile, RelationTarget, stable_id};
use crate::schema::{
    CODE_GRAPH_SCHEMA_VERSION, ChangeKind, CodeEntity, CodeEntityKind, CodeGraphChangeImpactReport,
    CodeGraphChangeImpactRequest, CodeGraphChangeTarget, CodeGraphEntityMetrics,
    CodeGraphImpactPathStep, CodeGraphImpactReport, CodeGraphImpactedEntity,
    CodeGraphPackageReference, CodeGraphPath, CodeGraphSearchResult, CodeGraphVerificationTarget,
    CodeLanguage, CodeRelation, CodeRelationKind, GraphDirection, ImpactRiskTier,
    PersistentGraphStats, RelationEvidenceKind, SourceSpan,
};
use anyhow::{Result, anyhow};
use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::time::Instant;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone)]
pub(crate) struct CodeGraphStore {
    path: PathBuf,
}

impl CodeGraphStore {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn connect(&self) -> Result<Connection> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let connection = Connection::open(&self.path)?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "NORMAL")?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        connection.execute_batch(SCHEMA)?;
        ensure_column(
            &connection,
            "relations",
            "evidence_kind",
            "TEXT NOT NULL DEFAULT 'inferred'",
        )?;
        ensure_column(
            &connection,
            "relation_history",
            "evidence_kind",
            "TEXT NOT NULL DEFAULT 'inferred'",
        )?;
        connection.execute(
            "INSERT OR REPLACE INTO metadata(key, value) VALUES('schema_version', ?1)",
            [CODE_GRAPH_SCHEMA_VERSION.to_string()],
        )?;
        connection.execute(
            "INSERT OR IGNORE INTO metadata(key, value) VALUES('graph_revision', '0')",
            [],
        )?;
        Ok(connection)
    }

    pub(crate) fn known_files(&self) -> Result<BTreeMap<PathBuf, FileFingerprint>> {
        let connection = self.connect()?;
        let mut statement = connection
            .prepare("SELECT path, size_bytes, modified_millis, content_hash FROM files")?;
        let rows = statement.query_map([], |row| {
            Ok((
                PathBuf::from(row.get::<_, String>(0)?),
                FileFingerprint {
                    size: row.get::<_, i64>(1)? as u64,
                    modified_millis: row.get::<_, i64>(2)? as u64,
                    content_hash: row.get(3)?,
                },
            ))
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub(crate) fn apply_index(
        &self,
        workspace: &Path,
        project_id: &str,
        changed_files: &[ExtractedFile],
        deleted_files: &[PathBuf],
    ) -> Result<()> {
        let mut connection = self.connect()?;
        let transaction = connection.transaction()?;
        let now = now_millis();
        ensure_project_entity(&transaction, workspace, project_id, now)?;

        for path in deleted_files {
            archive_file(&transaction, path, now)?;
            transaction.execute("DELETE FROM files WHERE path = ?1", [path_text(path)])?;
        }

        for file in changed_files {
            archive_file(&transaction, &file.path, now)?;
            transaction.execute(
                "INSERT INTO files(path, content_hash, language, size_bytes, modified_millis, indexed_at) \
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6) \
                 ON CONFLICT(path) DO UPDATE SET content_hash=excluded.content_hash, \
                 language=excluded.language, size_bytes=excluded.size_bytes, \
                 modified_millis=excluded.modified_millis, indexed_at=excluded.indexed_at",
                params![
                    path_text(&file.path),
                    file.hash,
                    file.language.as_str(),
                    file.size as i64,
                    file.modified_millis as i64,
                    now,
                ],
            )?;
            for entity in &file.entities {
                let revision = next_entity_revision(&transaction, &entity.id)?;
                transaction.execute(
                    "INSERT INTO entities(id, revision, kind, name, qualified_name, language, file_path, \
                     start_byte, end_byte, start_line, start_column, end_line, end_column, valid_from) \
                     VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
                    params![
                        entity.id,
                        revision as i64,
                        entity.kind.as_str(),
                        entity.name,
                        entity.qualified_name,
                        entity.language.as_str(),
                        path_text(&entity.file_path),
                        entity.span.start_byte as i64,
                        entity.span.end_byte as i64,
                        entity.span.start_line as i64,
                        entity.span.start_column as i64,
                        entity.span.end_line as i64,
                        entity.span.end_column as i64,
                        now,
                    ],
                )?;
                insert_fts(
                    &transaction,
                    &entity.id,
                    &entity.name,
                    &entity.qualified_name,
                    &entity.file_path,
                )?;
            }
        }

        let (qualified_index, name_index) = entity_indexes(&transaction)?;
        for file in changed_files {
            for relation in &file.relations {
                let (target_id, evidence_kind) = match &relation.target {
                    RelationTarget::Id(id) => (id.clone(), RelationEvidenceKind::Observed),
                    RelationTarget::Name(name) => (
                        resolve_target(
                            &transaction,
                            name,
                            file.language,
                            &qualified_index,
                            &name_index,
                            now,
                        )?,
                        RelationEvidenceKind::Inferred,
                    ),
                    RelationTarget::TypedName(kind, name) => (
                        resolve_typed_target(&transaction, *kind, name, file.language, now)?,
                        RelationEvidenceKind::Observed,
                    ),
                };
                let relation_id = stable_id(
                    "relation",
                    &[
                        &relation.source_id,
                        &target_id,
                        relation.kind.as_str(),
                        &path_text(&relation.evidence_file),
                        &relation.evidence_span.start_line.to_string(),
                    ],
                );
                let revision = next_relation_revision(&transaction, &relation_id)?;
                transaction.execute(
                    "INSERT OR IGNORE INTO relations(id, revision, source_id, target_id, kind, confidence, evidence_kind, \
                     evidence_file, start_byte, end_byte, start_line, start_column, end_line, end_column, extractor, valid_from) \
                     VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
                    params![
                        relation_id,
                        revision as i64,
                        relation.source_id,
                        target_id,
                        relation.kind.as_str(),
                        relation.confidence,
                        evidence_kind.as_str(),
                        path_text(&relation.evidence_file),
                        relation.evidence_span.start_byte as i64,
                        relation.evidence_span.end_byte as i64,
                        relation.evidence_span.start_line as i64,
                        relation.evidence_span.start_column as i64,
                        relation.evidence_span.end_line as i64,
                        relation.evidence_span.end_column as i64,
                        relation.extractor,
                        now,
                    ],
                )?;
            }
        }

        if !changed_files.is_empty() || !deleted_files.is_empty() {
            transaction.execute(
                "UPDATE metadata SET value = CAST(value AS INTEGER) + 1 WHERE key = 'graph_revision'",
                [],
            )?;
            let graph_revision = transaction
                .query_row(
                    "SELECT value FROM metadata WHERE key = 'graph_revision'",
                    [],
                    |row| row.get::<_, String>(0),
                )?
                .parse()
                .unwrap_or_default();
            refresh_entity_metrics(&transaction, graph_revision)?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub(crate) fn graph_revision(&self) -> Result<u64> {
        let connection = self.connect()?;
        let value: String = connection.query_row(
            "SELECT value FROM metadata WHERE key = 'graph_revision'",
            [],
            |row| row.get(0),
        )?;
        Ok(value.parse().unwrap_or_default())
    }

    pub(crate) fn stats(&self) -> Result<PersistentGraphStats> {
        let connection = self.connect()?;
        let graph_revision = connection
            .query_row(
                "SELECT value FROM metadata WHERE key = 'graph_revision'",
                [],
                |row| row.get::<_, String>(0),
            )?
            .parse()
            .unwrap_or_default();
        Ok(PersistentGraphStats {
            schema_version: CODE_GRAPH_SCHEMA_VERSION,
            graph_revision,
            database_path: self.path.clone(),
            active_files: count(&connection, "files")?,
            active_entities: count(&connection, "entities")?,
            active_relations: count(&connection, "relations")?,
            historical_entities: count(&connection, "entity_history")?,
            historical_relations: count(&connection, "relation_history")?,
            database_bytes: std::fs::metadata(&self.path)
                .map(|meta| meta.len())
                .unwrap_or(0),
        })
    }

    pub(crate) fn entity_metrics(&self, entity_id: &str) -> Result<CodeGraphEntityMetrics> {
        let connection = self.connect()?;
        load_or_compute_metrics(&connection, entity_id)
    }

    pub(crate) fn analyze_change(
        &self,
        request: &CodeGraphChangeImpactRequest,
    ) -> Result<CodeGraphChangeImpactReport> {
        let started = Instant::now();
        anyhow::ensure!(
            !request.targets.is_empty(),
            "change impact requires at least one target"
        );
        let connection = self.connect()?;
        let graph_revision = metadata_u64(&connection, "graph_revision")?;
        let max_depth = request.max_depth.clamp(1, 8);
        let max_entities = request.max_entities.clamp(1, 2_000);
        let mut roots = Vec::new();
        let mut root_kinds = HashMap::<String, ChangeKind>::new();
        let mut unresolved_targets = Vec::new();

        for target in &request.targets {
            let resolved = resolve_change_target_entities(&connection, target, 16)?;
            if resolved.is_empty() {
                unresolved_targets.push(change_target_label(target));
                continue;
            }
            for entity in resolved {
                root_kinds
                    .entry(entity.id.clone())
                    .and_modify(|kind| {
                        if target.change_kind.risk_multiplier() > kind.risk_multiplier() {
                            *kind = target.change_kind;
                        }
                    })
                    .or_insert(target.change_kind);
                if !roots.iter().any(|root: &CodeEntity| root.id == entity.id) {
                    roots.push(entity);
                }
            }
        }

        let mut best = HashMap::<String, ImpactTraversalState>::new();
        let mut root_hits = HashMap::<String, BTreeSet<String>>::new();
        let mut frontier = BinaryHeap::<ImpactQueueEntry>::new();
        let mut queue_serial = 0_u64;
        for root in &roots {
            let change_kind = root_kinds.get(&root.id).copied().unwrap_or_default();
            let metrics = load_or_compute_metrics(&connection, &root.id)?;
            let score = change_kind.risk_multiplier()
                * root_kind_risk(root.kind)
                * (1.0 + metrics.centrality.min(8.0) * 0.08);
            let state = ImpactTraversalState {
                entity_id: root.id.clone(),
                distance: 0,
                propagation_score: score,
                root_id: root.id.clone(),
                path: Vec::new(),
            };
            root_hits
                .entry(root.id.clone())
                .or_default()
                .insert(root.id.clone());
            best.insert(root.id.clone(), state.clone());
            frontier.push(ImpactQueueEntry::new(queue_serial, state));
            queue_serial = queue_serial.saturating_add(1);
        }

        while let Some(entry) = frontier.pop() {
            if best.len() >= max_entities.saturating_add(roots.len()) {
                break;
            }
            let current = entry.state;
            if best.get(&current.entity_id).is_some_and(|known| {
                current.propagation_score + f64::EPSILON < known.propagation_score
            }) {
                continue;
            }
            if current.distance >= max_depth {
                continue;
            }
            for relation in load_adjacent_relations(
                &connection,
                &current.entity_id,
                GraphDirection::Both,
                &BTreeSet::new(),
            )? {
                if !request.include_inferred
                    && relation.evidence_kind == RelationEvidenceKind::Inferred
                {
                    continue;
                }
                let Some((neighbor, propagation_weight)) =
                    impact_neighbor(&relation, &current.entity_id)
                else {
                    continue;
                };
                let distance = current.distance + 1;
                root_hits
                    .entry(neighbor.clone())
                    .or_default()
                    .insert(current.root_id.clone());
                let evidence_weight = if relation.evidence_kind == RelationEvidenceKind::Observed {
                    1.0
                } else {
                    0.72
                };
                let score = current.propagation_score
                    * propagation_weight
                    * f64::from(relation.confidence).clamp(0.05, 1.0)
                    * evidence_weight
                    * (0.86_f64).powi(distance as i32);
                if score < 0.025 {
                    continue;
                }
                let should_update = best
                    .get(&neighbor)
                    .map(|existing| score > existing.propagation_score * 1.03)
                    .unwrap_or(true);
                if !should_update {
                    continue;
                }
                let mut path = current.path.clone();
                path.push(CodeGraphImpactPathStep {
                    from_entity_id: current.entity_id.clone(),
                    to_entity_id: neighbor.clone(),
                    relation: relation.kind,
                    confidence: relation.confidence,
                    evidence_kind: relation.evidence_kind,
                    evidence_file: relation.evidence_file.clone(),
                    evidence_span: relation.evidence_span.clone(),
                });
                let next = ImpactTraversalState {
                    entity_id: neighbor.clone(),
                    distance,
                    propagation_score: score,
                    root_id: current.root_id.clone(),
                    path,
                };
                best.insert(neighbor, next.clone());
                frontier.push(ImpactQueueEntry::new(queue_serial, next));
                queue_serial = queue_serial.saturating_add(1);
            }
        }

        let root_ids = roots
            .iter()
            .map(|root| root.id.as_str())
            .collect::<BTreeSet<_>>();
        let mut affected_entities = Vec::new();
        let mut affected_files = BTreeSet::new();
        let mut verification = BTreeMap::<PathBuf, CodeGraphVerificationTarget>::new();
        let mut public_api_entities = Vec::new();
        let mut external_dependencies = Vec::new();
        for state in best.values() {
            if root_ids.contains(state.entity_id.as_str()) {
                continue;
            }
            let entity = load_entity(&connection, &state.entity_id)?;
            let metrics = load_or_compute_metrics(&connection, &state.entity_id)?;
            let root_coverage = root_hits
                .get(&state.entity_id)
                .map(BTreeSet::len)
                .unwrap_or(1);
            let impact_score = state.propagation_score
                * (1.0 + metrics.centrality.min(10.0) * 0.05)
                * (1.0 + (root_coverage as f64).ln_1p() * 0.08);
            let risk_tier = risk_tier(impact_score);
            let mut reasons = vec![format!(
                "Reached in {} graph hop(s) from {} with weighted propagation {:.3}",
                state.distance, state.root_id, state.propagation_score
            )];
            if root_coverage > 1 {
                reasons.push(format!(
                    "Converging impact from {root_coverage} changed roots"
                ));
            }
            if metrics.inbound_count >= 8 {
                reasons.push(format!(
                    "High fan-in: {} inbound relationships",
                    metrics.inbound_count
                ));
            }
            if metrics.test_relation_count > 0 {
                reasons.push(format!(
                    "Covered by {} explicit test relationships",
                    metrics.test_relation_count
                ));
            }
            if !entity.file_path.as_os_str().is_empty() {
                affected_files.insert(entity.file_path.clone());
            }
            if is_test_entity(&entity) {
                let entry = verification
                    .entry(entity.file_path.clone())
                    .or_insert_with(|| CodeGraphVerificationTarget {
                        file_path: entity.file_path.clone(),
                        reason: "Directly connected test evidence in the code graph".to_owned(),
                        confidence: impact_score.min(1.0),
                        related_entity_ids: Vec::new(),
                    });
                entry.confidence = entry.confidence.max(impact_score.min(1.0));
                entry.related_entity_ids.push(entity.id.clone());
            }
            if is_public_api_candidate(&entity, &metrics) {
                public_api_entities.push(entity.clone());
            }
            if entity.kind == CodeEntityKind::External || entity.file_path.as_os_str().is_empty() {
                external_dependencies.push(entity.clone());
            }
            affected_entities.push(CodeGraphImpactedEntity {
                entity,
                metrics,
                distance: state.distance,
                impact_score,
                risk_tier,
                reasons,
                path: state.path.clone(),
            });
        }
        affected_entities.sort_by(|left, right| {
            right
                .impact_score
                .total_cmp(&left.impact_score)
                .then_with(|| left.distance.cmp(&right.distance))
                .then_with(|| left.entity.qualified_name.cmp(&right.entity.qualified_name))
        });
        public_api_entities.sort_by(|left, right| left.qualified_name.cmp(&right.qualified_name));
        public_api_entities.dedup_by(|left, right| left.id == right.id);
        external_dependencies.sort_by(|left, right| left.qualified_name.cmp(&right.qualified_name));
        external_dependencies.dedup_by(|left, right| left.id == right.id);
        let aggregate_risk_score = affected_entities
            .first()
            .map(|entity| entity.impact_score)
            .unwrap_or_else(|| {
                roots
                    .iter()
                    .map(|root| root_kind_risk(root.kind))
                    .fold(0.0, f64::max)
            })
            + ((affected_entities.len() as f64 + 1.0).ln() * 0.12)
            + (public_api_entities.len() as f64 * 0.08);

        Ok(CodeGraphChangeImpactReport {
            schema_version: CODE_GRAPH_SCHEMA_VERSION,
            graph_revision,
            targets: request.targets.clone(),
            roots,
            risk_tier: risk_tier(aggregate_risk_score),
            aggregate_risk_score,
            affected_entities,
            affected_files: affected_files.into_iter().collect(),
            verification_targets: verification.into_values().collect(),
            public_api_entities,
            external_dependencies,
            unresolved_targets,
            analysis_millis: started.elapsed().as_millis(),
        })
    }

    pub(crate) fn representative_entities(&self, limit: usize) -> Result<Vec<CodeEntity>> {
        let connection = self.connect()?;
        let mut statement = connection.prepare(
            "SELECT id, revision, kind, name, qualified_name, language, file_path, \
             start_byte, end_byte, start_line, start_column, end_line, end_column, valid_from \
             FROM entities WHERE file_path <> '' AND kind IN \
             ('module', 'namespace', 'class', 'struct', 'enum', 'trait', 'interface', 'function') \
             ORDER BY CASE kind \
               WHEN 'module' THEN 0 WHEN 'namespace' THEN 0 \
               WHEN 'class' THEN 1 WHEN 'struct' THEN 1 WHEN 'trait' THEN 1 \
               WHEN 'interface' THEN 1 WHEN 'enum' THEN 2 ELSE 3 END, \
             qualified_name LIMIT ?1",
        )?;
        let rows = statement.query_map([limit.max(1) as i64], |row| entity_from_row(row, 0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub(crate) fn package_references(&self) -> Result<Vec<CodeGraphPackageReference>> {
        let connection = self.connect()?;
        let mut statement = connection.prepare(
            "SELECT e.qualified_name, COUNT(r.id) \
             FROM entities e LEFT JOIN relations r \
               ON r.source_id = e.id AND r.kind = 'depends_on' \
             WHERE e.kind = 'package' \
             GROUP BY e.id, e.qualified_name \
             ORDER BY COUNT(r.id) DESC, e.qualified_name",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(CodeGraphPackageReference {
                label: row.get(0)?,
                outbound_references: row.get::<_, i64>(1)? as usize,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub(crate) fn search(&self, term: &str, limit: usize) -> Result<Vec<CodeGraphSearchResult>> {
        let connection = self.connect()?;
        let query = term
            .split_whitespace()
            .filter(|part| !part.is_empty())
            .map(|part| format!("\"{}\"*", part.replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(" AND ");
        if query.is_empty() {
            return Ok(Vec::new());
        }
        let graph_revision = metadata_u64(&connection, "graph_revision")?;
        let mut statement = connection.prepare(
            "SELECT bm25(entities_fts), e.id, e.revision, e.kind, e.name, e.qualified_name, \
             e.language, e.file_path, e.start_byte, e.end_byte, e.start_line, e.start_column, \
             e.end_line, e.end_column, e.valid_from \
             FROM entities_fts JOIN entities e ON e.id = entities_fts.id \
             WHERE entities_fts MATCH ?1 ORDER BY bm25(entities_fts) LIMIT ?2",
        )?;
        let rows = statement.query_map(params![query, limit.max(1) as i64], |row| {
            Ok(CodeGraphSearchResult {
                graph_revision,
                score: -row.get::<_, f64>(0)?,
                entity: entity_from_row(row, 1)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub(crate) fn impact(
        &self,
        term: &str,
        depth: usize,
        limit: usize,
    ) -> Result<CodeGraphImpactReport> {
        self.traverse(term, GraphDirection::Inbound, &[], depth, limit)
    }

    pub(crate) fn traverse(
        &self,
        term: &str,
        direction: GraphDirection,
        relation_kinds: &[CodeRelationKind],
        depth: usize,
        limit: usize,
    ) -> Result<CodeGraphImpactReport> {
        let root = self
            .search(term, 1)?
            .into_iter()
            .next()
            .map(|result| result.entity)
            .ok_or_else(|| anyhow!("no persistent graph entity matched '{term}'"))?;
        let connection = self.connect()?;
        let graph_revision = metadata_u64(&connection, "graph_revision")?;
        let requested_kinds = relation_kinds.iter().copied().collect::<BTreeSet<_>>();
        let entity_limit = limit.max(1);
        let mut queue = VecDeque::from([(root.id.clone(), 0usize)]);
        let mut visited = BTreeSet::from([root.id.clone()]);
        let mut entity_ids = vec![root.id.clone()];
        let mut relation_ids = BTreeSet::new();
        let mut selected_relations = Vec::new();

        while let Some((current, current_depth)) = queue.pop_front() {
            if current_depth >= depth || entity_ids.len() >= entity_limit {
                continue;
            }
            for relation in
                load_adjacent_relations(&connection, &current, direction, &requested_kinds)?
            {
                let Some(next) = next_entity_for_direction(&relation, &current, direction) else {
                    continue;
                };
                if visited.insert(next.clone()) {
                    entity_ids.push(next.clone());
                    if relation_ids.insert(relation.id.clone()) {
                        selected_relations.push(relation);
                    }
                    if entity_ids.len() >= entity_limit {
                        break;
                    }
                    queue.push_back((next, current_depth + 1));
                }
            }
        }

        let entities = entity_ids
            .iter()
            .map(|id| load_entity(&connection, id))
            .collect::<Result<Vec<_>>>()?;
        let used_kinds = selected_relations
            .iter()
            .map(|relation| relation.kind)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        Ok(CodeGraphImpactReport {
            graph_revision,
            root,
            direction,
            relation_kinds: used_kinds,
            depth,
            entities,
            relations: selected_relations,
        })
    }

    pub(crate) fn find_path(
        &self,
        from: &str,
        to: &str,
        max_depth: usize,
    ) -> Result<Option<CodeGraphPath>> {
        self.find_path_filtered(from, to, max_depth, GraphDirection::Outbound, &[])
    }

    pub(crate) fn find_path_filtered(
        &self,
        from: &str,
        to: &str,
        max_depth: usize,
        direction: GraphDirection,
        relation_kinds: &[CodeRelationKind],
    ) -> Result<Option<CodeGraphPath>> {
        let source = self
            .search(from, 1)?
            .into_iter()
            .next()
            .map(|result| result.entity)
            .ok_or_else(|| anyhow!("no persistent graph entity matched '{from}'"))?;
        let target = self
            .search(to, 1)?
            .into_iter()
            .next()
            .map(|result| result.entity)
            .ok_or_else(|| anyhow!("no persistent graph entity matched '{to}'"))?;
        let connection = self.connect()?;
        let graph_revision = metadata_u64(&connection, "graph_revision")?;
        let requested_kinds = relation_kinds.iter().copied().collect::<BTreeSet<_>>();
        let mut queue = VecDeque::from([(source.id.clone(), Vec::<String>::new())]);
        let mut visited = BTreeSet::new();

        while let Some((current, relation_path)) = queue.pop_front() {
            if current == target.id {
                let relations = relation_path
                    .iter()
                    .map(|id| load_relation(&connection, id))
                    .collect::<Result<Vec<_>>>()?;
                let mut entity_ids = vec![source.id.clone()];
                for relation in &relations {
                    let previous = entity_ids.last().expect("path has source");
                    let next = next_entity_for_direction(relation, previous, direction)
                        .ok_or_else(|| anyhow!("stored path violates requested direction"))?;
                    entity_ids.push(next);
                }
                let entities = entity_ids
                    .iter()
                    .map(|id| load_entity(&connection, id))
                    .collect::<Result<Vec<_>>>()?;
                let used_kinds = relations
                    .iter()
                    .map(|relation| relation.kind)
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect();
                return Ok(Some(CodeGraphPath {
                    graph_revision,
                    direction,
                    relation_kinds: used_kinds,
                    entities,
                    relations,
                }));
            }
            if !visited.insert(current.clone()) || relation_path.len() >= max_depth {
                continue;
            }
            for relation in
                load_adjacent_relations(&connection, &current, direction, &requested_kinds)?
            {
                let Some(next) = next_entity_for_direction(&relation, &current, direction) else {
                    continue;
                };
                if !visited.contains(&next) {
                    let mut next_path = relation_path.clone();
                    next_path.push(relation.id);
                    queue.push_back((next, next_path));
                }
            }
        }
        Ok(None)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct FileFingerprint {
    pub(crate) size: u64,
    pub(crate) modified_millis: u64,
    pub(crate) content_hash: String,
}

fn ensure_project_entity(
    transaction: &Transaction<'_>,
    workspace: &Path,
    project_id: &str,
    now: i64,
) -> Result<()> {
    let name = workspace
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("project");
    transaction.execute(
        "INSERT OR IGNORE INTO entities(id, revision, kind, name, qualified_name, language, file_path, \
         start_byte, end_byte, start_line, start_column, end_line, end_column, valid_from) \
         VALUES(?1, 1, 'project', ?2, ?3, 'project', '', 0, 0, 1, 1, 1, 1, ?4)",
        params![project_id, name, workspace.to_string_lossy(), now],
    )?;
    insert_fts(
        transaction,
        project_id,
        name,
        &workspace.to_string_lossy(),
        Path::new(""),
    )?;
    Ok(())
}

fn archive_file(transaction: &Transaction<'_>, path: &Path, now: i64) -> Result<()> {
    let path = path_text(path);
    let mut statement = transaction.prepare("SELECT id FROM entities WHERE file_path = ?1")?;
    let ids = statement
        .query_map([&path], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(statement);

    // Archive every relation whose evidence lives in the file OR whose source/target entity
    // is being replaced. Without this, incremental re-indexing can leave dangling edges from
    // unchanged files to a stale entity revision and inflate future blast-radius reports.
    transaction.execute(
        "INSERT OR IGNORE INTO relation_history SELECT id, revision, source_id, target_id, kind, confidence, evidence_kind, \
         evidence_file, start_byte, end_byte, start_line, start_column, end_line, end_column, extractor, valid_from, ?2 \
         FROM relations WHERE evidence_file = ?1 \
         OR source_id IN (SELECT id FROM entities WHERE file_path = ?1) \
         OR target_id IN (SELECT id FROM entities WHERE file_path = ?1)",
        params![path, now],
    )?;
    transaction.execute(
        "DELETE FROM relations WHERE evidence_file = ?1 \
         OR source_id IN (SELECT id FROM entities WHERE file_path = ?1) \
         OR target_id IN (SELECT id FROM entities WHERE file_path = ?1)",
        [&path],
    )?;

    for id in &ids {
        transaction.execute("DELETE FROM entities_fts WHERE id = ?1", [id])?;
        transaction.execute("DELETE FROM entity_metrics WHERE entity_id = ?1", [id])?;
    }
    transaction.execute(
        "INSERT OR IGNORE INTO entity_history SELECT id, revision, kind, name, qualified_name, language, file_path, \
         start_byte, end_byte, start_line, start_column, end_line, end_column, valid_from, ?2 \
         FROM entities WHERE file_path = ?1",
        params![path, now],
    )?;
    transaction.execute("DELETE FROM entities WHERE file_path = ?1", [&path])?;
    Ok(())
}

struct ImpactQueueEntry {
    serial: u64,
    state: ImpactTraversalState,
}

impl ImpactQueueEntry {
    fn new(serial: u64, state: ImpactTraversalState) -> Self {
        Self { serial, state }
    }
}

impl PartialEq for ImpactQueueEntry {
    fn eq(&self, other: &Self) -> bool {
        self.serial == other.serial
            && self.state.propagation_score.to_bits() == other.state.propagation_score.to_bits()
    }
}

impl Eq for ImpactQueueEntry {}

impl PartialOrd for ImpactQueueEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ImpactQueueEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        self.state
            .propagation_score
            .total_cmp(&other.state.propagation_score)
            .then_with(|| other.serial.cmp(&self.serial))
    }
}

#[derive(Clone)]
struct ImpactTraversalState {
    entity_id: String,
    distance: usize,
    propagation_score: f64,
    root_id: String,
    path: Vec<CodeGraphImpactPathStep>,
}

fn refresh_entity_metrics(transaction: &Transaction<'_>, graph_revision: u64) -> Result<()> {
    let mut inbound = HashMap::<String, (usize, usize, usize)>::new();
    {
        let mut statement = transaction.prepare(
            "SELECT target_id, COUNT(*), SUM(CASE WHEN evidence_kind='observed' THEN 1 ELSE 0 END), SUM(CASE WHEN kind='tests' THEN 1 ELSE 0 END) FROM relations GROUP BY target_id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)? as usize,
                row.get::<_, i64>(2)? as usize,
                row.get::<_, i64>(3)? as usize,
            ))
        })?;
        for row in rows {
            let (id, count, observed, tests) = row?;
            inbound.insert(id, (count, observed, tests));
        }
    }
    let mut outbound = HashMap::<String, usize>::new();
    {
        let mut statement =
            transaction.prepare("SELECT source_id, COUNT(*) FROM relations GROUP BY source_id")?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as usize))
        })?;
        for row in rows {
            let (id, count) = row?;
            outbound.insert(id, count);
        }
    }
    transaction.execute("DELETE FROM entity_metrics", [])?;
    let entities = {
        let mut statement = transaction.prepare("SELECT id, kind FROM entities")?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    for (id, kind_text) in entities {
        let kind = CodeEntityKind::parse(&kind_text).unwrap_or(CodeEntityKind::External);
        let (inbound_count, observed_count, test_count) =
            inbound.get(&id).copied().unwrap_or_default();
        let outbound_count = outbound.get(&id).copied().unwrap_or_default();
        let centrality = ((inbound_count as f64 * 2.4)
            + (outbound_count as f64 * 0.8)
            + (observed_count as f64 * 0.35)
            + 1.0)
            .ln();
        let change_risk = root_kind_risk(kind) * (1.0 + centrality * 0.12);
        transaction.execute(
            "INSERT INTO entity_metrics(entity_id, graph_revision, inbound_count, outbound_count, observed_relation_count, test_relation_count, centrality, change_risk) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![id, graph_revision as i64, inbound_count as i64, outbound_count as i64, observed_count as i64, test_count as i64, centrality, change_risk],
        )?;
    }
    Ok(())
}

fn load_or_compute_metrics(
    connection: &Connection,
    entity_id: &str,
) -> Result<CodeGraphEntityMetrics> {
    if let Some(metrics) = connection
        .query_row(
            "SELECT entity_id, graph_revision, inbound_count, outbound_count, observed_relation_count, test_relation_count, centrality, change_risk FROM entity_metrics WHERE entity_id=?1",
            [entity_id],
            |row| {
                Ok(CodeGraphEntityMetrics {
                    entity_id: row.get(0)?,
                    graph_revision: row.get::<_, i64>(1)? as u64,
                    inbound_count: row.get::<_, i64>(2)? as usize,
                    outbound_count: row.get::<_, i64>(3)? as usize,
                    observed_relation_count: row.get::<_, i64>(4)? as usize,
                    test_relation_count: row.get::<_, i64>(5)? as usize,
                    centrality: row.get(6)?,
                    change_risk: row.get(7)?,
                })
            },
        )
        .optional()?
    {
        return Ok(metrics);
    }
    let graph_revision = metadata_u64(connection, "graph_revision")?;
    let inbound_count = connection.query_row(
        "SELECT COUNT(*) FROM relations WHERE target_id=?1",
        [entity_id],
        |row| row.get::<_, i64>(0),
    )? as usize;
    let outbound_count = connection.query_row(
        "SELECT COUNT(*) FROM relations WHERE source_id=?1",
        [entity_id],
        |row| row.get::<_, i64>(0),
    )? as usize;
    let observed_relation_count = connection.query_row("SELECT COUNT(*) FROM relations WHERE (source_id=?1 OR target_id=?1) AND evidence_kind='observed'", [entity_id], |row| row.get::<_, i64>(0))? as usize;
    let test_relation_count = connection.query_row(
        "SELECT COUNT(*) FROM relations WHERE target_id=?1 AND kind='tests'",
        [entity_id],
        |row| row.get::<_, i64>(0),
    )? as usize;
    let entity = load_entity(connection, entity_id)?;
    let centrality = ((inbound_count as f64 * 2.4)
        + (outbound_count as f64 * 0.8)
        + (observed_relation_count as f64 * 0.35)
        + 1.0)
        .ln();
    Ok(CodeGraphEntityMetrics {
        entity_id: entity_id.to_owned(),
        graph_revision,
        inbound_count,
        outbound_count,
        observed_relation_count,
        test_relation_count,
        centrality,
        change_risk: root_kind_risk(entity.kind) * (1.0 + centrality * 0.12),
    })
}

fn resolve_change_target_entities(
    connection: &Connection,
    target: &CodeGraphChangeTarget,
    limit: usize,
) -> Result<Vec<CodeEntity>> {
    let path = target.file_path.as_ref().map(|path| path_text(path));
    let symbol = target
        .symbol
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(path) = path {
        let has_span = target.start_line.is_some() || target.end_line.is_some();
        let mut sql = String::from(
            "SELECT id, revision, kind, name, qualified_name, language, file_path, start_byte, end_byte, start_line, start_column, end_line, end_column, valid_from FROM entities WHERE file_path=?1",
        );
        if has_span {
            sql.push_str(" AND end_line >= ?2 AND start_line <= ?3");
        }
        if symbol.is_some() {
            sql.push_str(if has_span {
                " AND (name=?4 OR qualified_name LIKE ?5)"
            } else {
                " AND (name=?2 OR qualified_name LIKE ?3)"
            });
        }
        sql.push_str(" ORDER BY CASE kind WHEN 'file' THEN 0 WHEN 'function' THEN 1 WHEN 'method' THEN 1 WHEN 'test' THEN 1 ELSE 2 END, (end_line-start_line) ASC LIMIT 32");
        let mut statement = connection.prepare(&sql)?;
        let values = match (has_span, symbol) {
            (true, Some(symbol)) => {
                let start = target.start_line.unwrap_or(1);
                let end = target.end_line.unwrap_or(start);
                statement
                    .query_map(
                        params![
                            path,
                            start as i64,
                            end as i64,
                            symbol,
                            format!("%{symbol}%")
                        ],
                        |row| entity_from_row(row, 0),
                    )?
                    .collect::<rusqlite::Result<Vec<_>>>()?
            }
            (true, None) => {
                let start = target.start_line.unwrap_or(1);
                let end = target.end_line.unwrap_or(start);
                statement
                    .query_map(params![path, start as i64, end as i64], |row| {
                        entity_from_row(row, 0)
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?
            }
            (false, Some(symbol)) => statement
                .query_map(params![path, symbol, format!("%{symbol}%")], |row| {
                    entity_from_row(row, 0)
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?,
            (false, None) => statement
                .query_map(params![path], |row| entity_from_row(row, 0))?
                .collect::<rusqlite::Result<Vec<_>>>()?,
        };
        return Ok(values);
    }
    if let Some(symbol) = symbol {
        let query = symbol
            .split_whitespace()
            .filter(|part| !part.is_empty())
            .map(|part| format!("\"{}\"*", part.replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(" AND ");
        if query.is_empty() {
            return Ok(Vec::new());
        }
        let mut statement = connection.prepare(
            "SELECT e.id, e.revision, e.kind, e.name, e.qualified_name, e.language, e.file_path, e.start_byte, e.end_byte, e.start_line, e.start_column, e.end_line, e.end_column, e.valid_from FROM entities_fts JOIN entities e ON e.id=entities_fts.id WHERE entities_fts MATCH ?1 ORDER BY CASE WHEN e.name=?2 THEN 0 WHEN e.qualified_name=?2 THEN 0 ELSE 1 END, bm25(entities_fts) LIMIT ?3",
        )?;
        let rows = statement.query_map(params![query, symbol, limit.max(1) as i64], |row| {
            entity_from_row(row, 0)
        })?;
        return Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?);
    }
    Ok(Vec::new())
}

fn change_target_label(target: &CodeGraphChangeTarget) -> String {
    match (&target.file_path, &target.symbol) {
        (Some(path), Some(symbol)) => format!("{}::{symbol}", path.display()),
        (Some(path), None) => path.display().to_string(),
        (None, Some(symbol)) => symbol.clone(),
        (None, None) => "<empty-target>".to_owned(),
    }
}

fn impact_neighbor(relation: &CodeRelation, current: &str) -> Option<(String, f64)> {
    let inbound = relation.target_id == current;
    let outbound = relation.source_id == current;
    let weight = match relation.kind {
        CodeRelationKind::Calls => {
            if inbound {
                1.0
            } else {
                0.22
            }
        }
        CodeRelationKind::Tests => {
            if inbound {
                1.08
            } else {
                0.18
            }
        }
        CodeRelationKind::References => {
            if inbound {
                0.88
            } else {
                0.2
            }
        }
        CodeRelationKind::Imports | CodeRelationKind::DependsOn => {
            if inbound {
                0.82
            } else {
                0.18
            }
        }
        CodeRelationKind::Implements | CodeRelationKind::Extends => {
            if inbound {
                0.96
            } else {
                0.38
            }
        }
        CodeRelationKind::RoutesTo => {
            if inbound {
                0.98
            } else {
                0.35
            }
        }
        CodeRelationKind::Reads | CodeRelationKind::UsesEnvironment | CodeRelationKind::Queries => {
            if inbound {
                0.86
            } else {
                0.18
            }
        }
        CodeRelationKind::Writes | CodeRelationKind::Mutates | CodeRelationKind::Configures => {
            if inbound {
                0.8
            } else {
                0.28
            }
        }
        CodeRelationKind::Emits | CodeRelationKind::Handles => 0.72,
        CodeRelationKind::Defines | CodeRelationKind::Contains => {
            if outbound {
                0.94
            } else {
                0.42
            }
        }
    };
    if inbound {
        Some((relation.source_id.clone(), weight))
    } else if outbound {
        Some((relation.target_id.clone(), weight))
    } else {
        None
    }
}

fn root_kind_risk(kind: CodeEntityKind) -> f64 {
    match kind {
        CodeEntityKind::Project | CodeEntityKind::Package => 1.35,
        CodeEntityKind::File | CodeEntityKind::Module | CodeEntityKind::Namespace => 1.15,
        CodeEntityKind::Trait | CodeEntityKind::Interface | CodeEntityKind::TypeAlias => 1.28,
        CodeEntityKind::Class | CodeEntityKind::Struct | CodeEntityKind::Enum => 1.2,
        CodeEntityKind::Function | CodeEntityKind::Method | CodeEntityKind::Implementation => 1.0,
        CodeEntityKind::Test => 0.72,
        CodeEntityKind::Route | CodeEntityKind::Event => 1.18,
        CodeEntityKind::EnvironmentVariable | CodeEntityKind::ConfigurationKey => 1.22,
        CodeEntityKind::DatabaseObject => 1.3,
        CodeEntityKind::Import | CodeEntityKind::Constant | CodeEntityKind::Variable => 0.9,
        CodeEntityKind::External => 1.05,
    }
}

fn risk_tier(score: f64) -> ImpactRiskTier {
    if score >= 1.75 {
        ImpactRiskTier::Critical
    } else if score >= 1.15 {
        ImpactRiskTier::High
    } else if score >= 0.58 {
        ImpactRiskTier::Medium
    } else {
        ImpactRiskTier::Low
    }
}

fn is_test_entity(entity: &CodeEntity) -> bool {
    entity.kind == CodeEntityKind::Test
        || entity.file_path.components().any(|component| {
            let value = component.as_os_str().to_string_lossy().to_ascii_lowercase();
            value == "tests"
                || value == "test"
                || value == "__tests__"
                || value.ends_with("_test.rs")
                || value.ends_with(".test.ts")
        })
        || entity.name.starts_with("test_")
}

fn is_public_api_candidate(entity: &CodeEntity, metrics: &CodeGraphEntityMetrics) -> bool {
    matches!(
        entity.kind,
        CodeEntityKind::Trait
            | CodeEntityKind::Interface
            | CodeEntityKind::Class
            | CodeEntityKind::Struct
            | CodeEntityKind::Enum
            | CodeEntityKind::TypeAlias
    ) || (matches!(
        entity.kind,
        CodeEntityKind::Function | CodeEntityKind::Method
    ) && metrics.inbound_count >= 4)
}

fn entity_indexes(transaction: &Transaction<'_>) -> Result<EntityIndexes> {
    let mut statement = transaction.prepare("SELECT id, name, qualified_name FROM entities")?;
    let mut qualified = HashMap::new();
    let mut names = HashMap::<String, Vec<String>>::new();
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    for row in rows {
        let (id, name, qualified_name) = row?;
        qualified.insert(qualified_name, id.clone());
        names.entry(name).or_default().push(id);
    }
    Ok((qualified, names))
}

type EntityIndexes = (HashMap<String, String>, HashMap<String, Vec<String>>);

fn resolve_typed_target(
    transaction: &Transaction<'_>,
    kind: CodeEntityKind,
    name: &str,
    language: CodeLanguage,
    now: i64,
) -> Result<String> {
    let id = stable_id(kind.as_str(), &[language.as_str(), name]);
    transaction.execute(
        "INSERT OR IGNORE INTO entities(id, revision, kind, name, qualified_name, language, file_path, start_byte, end_byte, start_line, start_column, end_line, end_column, valid_from) VALUES(?1, 1, ?2, ?3, ?3, ?4, '', 0, 0, 1, 1, 1, 1, ?5)",
        params![id, kind.as_str(), name, language.as_str(), now],
    )?;
    insert_fts(transaction, &id, name, name, Path::new(""))?;
    Ok(id)
}

fn resolve_target(
    transaction: &Transaction<'_>,
    name: &str,
    language: CodeLanguage,
    qualified: &HashMap<String, String>,
    names: &HashMap<String, Vec<String>>,
    now: i64,
) -> Result<String> {
    if let Some(id) = qualified.get(name) {
        return Ok(id.clone());
    }
    if let Some(candidates) = names.get(name)
        && candidates.len() == 1
    {
        return Ok(candidates[0].clone());
    }
    let id = stable_id("external", &[language.as_str(), name]);
    transaction.execute(
        "INSERT OR IGNORE INTO entities(id, revision, kind, name, qualified_name, language, file_path, \
         start_byte, end_byte, start_line, start_column, end_line, end_column, valid_from) \
         VALUES(?1, 1, 'external', ?2, ?2, ?3, '', 0, 0, 1, 1, 1, 1, ?4)",
        params![id, name, language.as_str(), now],
    )?;
    insert_fts(transaction, &id, name, name, Path::new(""))?;
    Ok(id)
}

fn insert_fts(
    transaction: &Transaction<'_>,
    id: &str,
    name: &str,
    qualified_name: &str,
    file_path: &Path,
) -> Result<()> {
    transaction.execute("DELETE FROM entities_fts WHERE id = ?1", [id])?;
    transaction.execute(
        "INSERT INTO entities_fts(id, name, qualified_name, file_path) VALUES(?1, ?2, ?3, ?4)",
        params![id, name, qualified_name, path_text(file_path)],
    )?;
    Ok(())
}

fn next_entity_revision(transaction: &Transaction<'_>, id: &str) -> Result<u64> {
    Ok(transaction.query_row(
        "SELECT COALESCE(MAX(revision), 0) + 1 FROM ( \
         SELECT revision FROM entities WHERE id=?1 UNION ALL \
         SELECT revision FROM entity_history WHERE id=?1)",
        [id],
        |row| row.get::<_, i64>(0),
    )? as u64)
}

fn next_relation_revision(transaction: &Transaction<'_>, id: &str) -> Result<u64> {
    Ok(transaction.query_row(
        "SELECT COALESCE(MAX(revision), 0) + 1 FROM ( \
         SELECT revision FROM relations WHERE id=?1 UNION ALL \
         SELECT revision FROM relation_history WHERE id=?1)",
        [id],
        |row| row.get::<_, i64>(0),
    )? as u64)
}

fn count(connection: &Connection, table: &str) -> Result<usize> {
    let sql = format!("SELECT COUNT(*) FROM {table}");
    Ok(connection.query_row(&sql, [], |row| row.get::<_, i64>(0))? as usize)
}

fn load_entity(connection: &Connection, id: &str) -> Result<CodeEntity> {
    connection
        .query_row(
            "SELECT id, revision, kind, name, qualified_name, language, file_path, start_byte, \
             end_byte, start_line, start_column, end_line, end_column, valid_from FROM entities WHERE id=?1",
            [id],
            |row| entity_from_row(row, 0),
        )
        .optional()?
        .ok_or_else(|| anyhow!("graph entity '{id}' no longer exists"))
}

fn load_relation(connection: &Connection, id: &str) -> Result<CodeRelation> {
    connection
        .query_row(
            "SELECT id, revision, source_id, target_id, kind, confidence, evidence_kind, evidence_file, start_byte, \
             end_byte, start_line, start_column, end_line, end_column, extractor, valid_from \
             FROM relations WHERE id=?1",
            [id],
            |row| relation_from_row(row, 0),
        )
        .optional()?
        .ok_or_else(|| anyhow!("graph relation '{id}' no longer exists"))
}

fn load_adjacent_relations(
    connection: &Connection,
    entity_id: &str,
    direction: GraphDirection,
    requested_kinds: &BTreeSet<CodeRelationKind>,
) -> Result<Vec<CodeRelation>> {
    const COLUMNS: &str = "id, revision, source_id, target_id, kind, confidence, evidence_kind, evidence_file, start_byte, end_byte, start_line, start_column, end_line, end_column, extractor, valid_from";
    let predicate = match direction {
        GraphDirection::Inbound => "target_id = ?1",
        GraphDirection::Outbound => "source_id = ?1",
        GraphDirection::Both => "source_id = ?1 OR target_id = ?1",
    };
    let sql = format!("SELECT {COLUMNS} FROM relations WHERE {predicate}");
    let mut statement = connection.prepare_cached(&sql)?;
    let rows = statement.query_map([entity_id], |row| relation_from_row(row, 0))?;
    let mut relations = Vec::new();
    for row in rows {
        let relation = row?;
        if requested_kinds.is_empty() || requested_kinds.contains(&relation.kind) {
            relations.push(relation);
        }
    }
    relations.sort_by(|left, right| {
        right
            .confidence
            .total_cmp(&left.confidence)
            .then_with(|| left.kind.cmp(&right.kind))
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(relations)
}

fn entity_from_row(row: &Row<'_>, offset: usize) -> rusqlite::Result<CodeEntity> {
    let kind = row.get::<_, String>(offset + 2)?;
    let language = row.get::<_, String>(offset + 5)?;
    Ok(CodeEntity {
        id: row.get(offset)?,
        revision: row.get::<_, i64>(offset + 1)? as u64,
        kind: CodeEntityKind::parse(&kind).unwrap_or(CodeEntityKind::External),
        name: row.get(offset + 3)?,
        qualified_name: row.get(offset + 4)?,
        language: CodeLanguage::parse(&language).unwrap_or(CodeLanguage::Project),
        file_path: PathBuf::from(row.get::<_, String>(offset + 6)?),
        span: SourceSpan {
            start_byte: row.get::<_, i64>(offset + 7)? as usize,
            end_byte: row.get::<_, i64>(offset + 8)? as usize,
            start_line: row.get::<_, i64>(offset + 9)? as usize,
            start_column: row.get::<_, i64>(offset + 10)? as usize,
            end_line: row.get::<_, i64>(offset + 11)? as usize,
            end_column: row.get::<_, i64>(offset + 12)? as usize,
        },
        valid_from_epoch_millis: row.get::<_, i64>(offset + 13)? as u128,
        valid_until_epoch_millis: None,
    })
}

fn relation_from_row(row: &Row<'_>, offset: usize) -> rusqlite::Result<CodeRelation> {
    let kind = row.get::<_, String>(offset + 4)?;
    Ok(CodeRelation {
        id: row.get(offset)?,
        revision: row.get::<_, i64>(offset + 1)? as u64,
        source_id: row.get(offset + 2)?,
        target_id: row.get(offset + 3)?,
        kind: CodeRelationKind::parse(&kind).unwrap_or(CodeRelationKind::References),
        confidence: row.get(offset + 5)?,
        evidence_kind: RelationEvidenceKind::parse(&row.get::<_, String>(offset + 6)?)
            .unwrap_or_default(),
        evidence_file: PathBuf::from(row.get::<_, String>(offset + 7)?),
        evidence_span: SourceSpan {
            start_byte: row.get::<_, i64>(offset + 8)? as usize,
            end_byte: row.get::<_, i64>(offset + 9)? as usize,
            start_line: row.get::<_, i64>(offset + 10)? as usize,
            start_column: row.get::<_, i64>(offset + 11)? as usize,
            end_line: row.get::<_, i64>(offset + 12)? as usize,
            end_column: row.get::<_, i64>(offset + 13)? as usize,
        },
        extractor: row.get(offset + 14)?,
        valid_from_epoch_millis: row.get::<_, i64>(offset + 15)? as u128,
        valid_until_epoch_millis: None,
    })
}

fn next_entity_for_direction(
    relation: &CodeRelation,
    current: &str,
    direction: GraphDirection,
) -> Option<String> {
    match direction {
        GraphDirection::Inbound if relation.target_id == current => {
            Some(relation.source_id.clone())
        }
        GraphDirection::Outbound if relation.source_id == current => {
            Some(relation.target_id.clone())
        }
        GraphDirection::Both if relation.source_id == current => Some(relation.target_id.clone()),
        GraphDirection::Both if relation.target_id == current => Some(relation.source_id.clone()),
        _ => None,
    }
}

fn metadata_u64(connection: &Connection, key: &str) -> Result<u64> {
    let value: String =
        connection.query_row("SELECT value FROM metadata WHERE key = ?1", [key], |row| {
            row.get(0)
        })?;
    Ok(value.parse().unwrap_or_default())
}

fn ensure_column(
    connection: &Connection,
    table: &str,
    column: &str,
    declaration: &str,
) -> Result<()> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<rusqlite::Result<BTreeSet<_>>>()?;
    if !columns.contains(column) {
        connection.execute_batch(&format!(
            "ALTER TABLE {table} ADD COLUMN {column} {declaration}"
        ))?;
    }
    Ok(())
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS metadata (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS files (
    path TEXT PRIMARY KEY,
    content_hash TEXT NOT NULL,
    language TEXT NOT NULL,
    size_bytes INTEGER NOT NULL,
    modified_millis INTEGER NOT NULL,
    indexed_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS entities (
    id TEXT PRIMARY KEY,
    revision INTEGER NOT NULL,
    kind TEXT NOT NULL,
    name TEXT NOT NULL,
    qualified_name TEXT NOT NULL,
    language TEXT NOT NULL,
    file_path TEXT NOT NULL,
    start_byte INTEGER NOT NULL,
    end_byte INTEGER NOT NULL,
    start_line INTEGER NOT NULL,
    start_column INTEGER NOT NULL,
    end_line INTEGER NOT NULL,
    end_column INTEGER NOT NULL,
    valid_from INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS entity_history (
    id TEXT NOT NULL,
    revision INTEGER NOT NULL,
    kind TEXT NOT NULL,
    name TEXT NOT NULL,
    qualified_name TEXT NOT NULL,
    language TEXT NOT NULL,
    file_path TEXT NOT NULL,
    start_byte INTEGER NOT NULL,
    end_byte INTEGER NOT NULL,
    start_line INTEGER NOT NULL,
    start_column INTEGER NOT NULL,
    end_line INTEGER NOT NULL,
    end_column INTEGER NOT NULL,
    valid_from INTEGER NOT NULL,
    valid_until INTEGER NOT NULL,
    PRIMARY KEY(id, revision)
);
CREATE TABLE IF NOT EXISTS relations (
    id TEXT PRIMARY KEY,
    revision INTEGER NOT NULL,
    source_id TEXT NOT NULL,
    target_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    confidence REAL NOT NULL,
    evidence_kind TEXT NOT NULL DEFAULT 'inferred',
    evidence_file TEXT NOT NULL,
    start_byte INTEGER NOT NULL,
    end_byte INTEGER NOT NULL,
    start_line INTEGER NOT NULL,
    start_column INTEGER NOT NULL,
    end_line INTEGER NOT NULL,
    end_column INTEGER NOT NULL,
    extractor TEXT NOT NULL,
    valid_from INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS relation_history (
    id TEXT NOT NULL,
    revision INTEGER NOT NULL,
    source_id TEXT NOT NULL,
    target_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    confidence REAL NOT NULL,
    evidence_kind TEXT NOT NULL DEFAULT 'inferred',
    evidence_file TEXT NOT NULL,
    start_byte INTEGER NOT NULL,
    end_byte INTEGER NOT NULL,
    start_line INTEGER NOT NULL,
    start_column INTEGER NOT NULL,
    end_line INTEGER NOT NULL,
    end_column INTEGER NOT NULL,
    extractor TEXT NOT NULL,
    valid_from INTEGER NOT NULL,
    valid_until INTEGER NOT NULL,
    PRIMARY KEY(id, revision)
);
CREATE TABLE IF NOT EXISTS entity_metrics (
    entity_id TEXT PRIMARY KEY,
    graph_revision INTEGER NOT NULL,
    inbound_count INTEGER NOT NULL,
    outbound_count INTEGER NOT NULL,
    observed_relation_count INTEGER NOT NULL,
    test_relation_count INTEGER NOT NULL,
    centrality REAL NOT NULL,
    change_risk REAL NOT NULL
);
CREATE VIRTUAL TABLE IF NOT EXISTS entities_fts USING fts5(
    id UNINDEXED, name, qualified_name, file_path, tokenize='unicode61'
);
CREATE INDEX IF NOT EXISTS idx_entities_name ON entities(name);
CREATE INDEX IF NOT EXISTS idx_entities_qualified ON entities(qualified_name);
CREATE INDEX IF NOT EXISTS idx_entities_file ON entities(file_path);
CREATE INDEX IF NOT EXISTS idx_relations_source ON relations(source_id);
CREATE INDEX IF NOT EXISTS idx_relations_target ON relations(target_id);
CREATE INDEX IF NOT EXISTS idx_relations_kind ON relations(kind);
CREATE INDEX IF NOT EXISTS idx_relations_evidence ON relations(evidence_file);
CREATE INDEX IF NOT EXISTS idx_relations_source_kind ON relations(source_id, kind);
CREATE INDEX IF NOT EXISTS idx_relations_target_kind ON relations(target_id, kind);
CREATE INDEX IF NOT EXISTS idx_entities_file_span ON entities(file_path, start_line, end_line);
CREATE INDEX IF NOT EXISTS idx_metrics_centrality ON entity_metrics(centrality DESC);
"#;
