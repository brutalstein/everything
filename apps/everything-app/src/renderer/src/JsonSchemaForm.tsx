type JsonSchemaProperty = {
  type?: "string" | "integer" | "number" | "boolean" | "array" | "object";
  title?: string;
  description?: string;
  format?: string;
  minimum?: number;
  maximum?: number;
  minLength?: number;
  maxLength?: number;
  enum?: string[];
  items?: { type?: string };
};

type JsonObjectSchema = {
  type?: string;
  properties?: Record<string, JsonSchemaProperty>;
  required?: string[];
};

type Props = {
  schema: unknown;
  value: string;
  onChange(value: string): void;
};

const labels: Record<string, string> = {
  query: "Arama",
  max_results: "En fazla sonuç",
  message_id: "Mesaj kimliği",
  to: "Alıcı",
  cc: "CC",
  bcc: "BCC",
  subject: "Konu",
  body: "Mesaj",
  reply_to_message_id: "Yanıtlanacak mesaj kimliği",
  thread_id: "Gmail konuşma kimliği",
  device_id: "Cihaz kimliği",
  uri: "Spotify URI",
  max_count: "En fazla video",
  video_url: "Video URL",
  privacy_level: "Gizlilik seviyesi",
  title: "Başlık",
  disable_comment: "Yorumları kapat",
  disable_duet: "Düeti kapat",
  disable_stitch: "Stitch'i kapat",
  is_aigc: "Yapay zekâ üretimi",
  publish_id: "Yayın kimliği",
  limit: "En fazla medya",
  image_url: "Görsel URL",
  caption: "Açıklama",
  owner: "Repository sahibi",
  repo: "Repository",
  state: "Durum",
  per_page: "Sayfa başına sonuç",
  number: "Issue / PR numarası",
  labels: "Etiketler",
  assignees: "Atanan kullanıcılar",
  head: "Kaynak branch",
  base: "Hedef branch",
  draft: "Taslak",
  merge_method: "Birleştirme yöntemi",
  commit_title: "Commit başlığı",
  commit_message: "Commit mesajı",
  workflow: "Workflow dosyası veya kimliği",
  ref: "Git ref",
  inputs: "Workflow girdileri (JSON)",
  path: "GitHub API yolu",
  method: "HTTP yöntemi",
  variables: "GraphQL değişkenleri (JSON)",
  sha: "Commit / branch referansı",
  kind: "Güvenlik uyarısı türü",
  tag_name: "Release etiketi",
  target_commitish: "Hedef commit / branch",
  name: "Ad",
  prerelease: "Ön sürüm",
  generate_release_notes: "Release notlarını otomatik üret",
  run_id: "Workflow çalışma kimliği",
};

function parseObject(value: string): Record<string, unknown> {
  try {
    const parsed = JSON.parse(value);
    return parsed && typeof parsed === "object" && !Array.isArray(parsed)
      ? parsed as Record<string, unknown>
      : {};
  } catch {
    return {};
  }
}

function fieldLabel(name: string, property: JsonSchemaProperty) {
  return property.title || labels[name] || name.replaceAll("_", " ");
}

function isLongText(name: string, property: JsonSchemaProperty) {
  return name === "body" || name === "caption" || (property.maxLength ?? 0) > 4_096;
}

export function JsonSchemaForm({ schema, value, onChange }: Props) {
  const typed = (schema && typeof schema === "object" ? schema : {}) as JsonObjectSchema;
  const properties = Object.entries(typed.properties ?? {});
  if (properties.length === 0) return <div className="field-hint">Bu eylem ek bilgi istemiyor.</div>;
  const current = parseObject(value);
  const required = new Set(typed.required ?? []);

  function update(name: string, next: unknown) {
    const updated = { ...current };
    if ((next === "" || next === undefined) && !required.has(name)) delete updated[name];
    else updated[name] = next;
    onChange(`${JSON.stringify(updated, null, 2)}\n`);
  }

  return (
    <div className="schema-form">
      {properties.map(([name, property]) => {
        const label = fieldLabel(name, property);
        const hint = property.description;
        const raw = current[name];
        if (property.enum?.length) {
          return (
            <label key={name}>
              {label}{required.has(name) ? " *" : ""}
              <select value={typeof raw === "string" ? raw : ""} onChange={(event) => update(name, event.target.value)}>
                <option value="">Seç</option>
                {property.enum.map((option) => <option key={option} value={option}>{option}</option>)}
              </select>
              {hint && <small className="field-hint">{hint}</small>}
            </label>
          );
        }
        if (property.type === "array") {
          const values = Array.isArray(raw) ? raw.map(String).join(", ") : "";
          return (
            <label key={name}>
              {label}{required.has(name) ? " *" : ""}
              <input value={values} placeholder="virgülle ayır" onChange={(event) => update(name, event.target.value.split(",").map((item) => item.trim()).filter(Boolean))} />
              {hint && <small className="field-hint">{hint}</small>}
            </label>
          );
        }
        if (property.type === "object") {
          return (
            <label key={name}>
              {label}{required.has(name) ? " *" : ""}
              <textarea
                rows={5}
                defaultValue={raw && typeof raw === "object" ? JSON.stringify(raw, null, 2) : "{}"}
                onBlur={(event) => {
                  try { update(name, JSON.parse(event.target.value || "{}")); } catch { event.target.setCustomValidity("Geçerli JSON gir"); event.target.reportValidity(); }
                }}
                onInput={(event) => event.currentTarget.setCustomValidity("")}
              />
              {hint && <small className="field-hint">{hint}</small>}
            </label>
          );
        }
        if (property.type === "boolean") {
          return (
            <label className="schema-checkbox" key={name}>
              <input
                type="checkbox"
                checked={Boolean(raw)}
                onChange={(event) => update(name, event.target.checked)}
              />
              <span>{label}{required.has(name) ? " *" : ""}</span>
            </label>
          );
        }
        if (property.type === "integer" || property.type === "number") {
          return (
            <label key={name}>
              {label}{required.has(name) ? " *" : ""}
              <input
                type="number"
                min={property.minimum}
                max={property.maximum}
                value={typeof raw === "number" ? raw : ""}
                onChange={(event) => update(name, event.target.value === "" ? "" : Number(event.target.value))}
              />
              {hint && <small className="field-hint">{hint}</small>}
            </label>
          );
        }
        if (isLongText(name, property)) {
          return (
            <label key={name}>
              {label}{required.has(name) ? " *" : ""}
              <textarea
                rows={name === "body" ? 8 : 4}
                maxLength={property.maxLength}
                value={typeof raw === "string" ? raw : ""}
                onChange={(event) => update(name, event.target.value)}
              />
              {hint && <small className="field-hint">{hint}</small>}
            </label>
          );
        }
        return (
          <label key={name}>
            {label}{required.has(name) ? " *" : ""}
            <input
              type={property.format === "email" ? "email" : property.format === "uri" ? "url" : "text"}
              minLength={property.minLength}
              maxLength={property.maxLength}
              value={typeof raw === "string" ? raw : ""}
              onChange={(event) => update(name, event.target.value)}
            />
            {hint && <small className="field-hint">{hint}</small>}
          </label>
        );
      })}
    </div>
  );
}
