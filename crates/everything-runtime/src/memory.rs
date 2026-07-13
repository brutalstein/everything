use crate::ModularRuntime;
use anyhow::Result;
use everything_domain::{MemoryEntry, MemoryQuery, MemorySearchResult, MemoryUpsertRequest};

impl ModularRuntime {
    pub fn remember(&self, request: MemoryUpsertRequest) -> Result<MemoryEntry> {
        self.memory_store.upsert(request)
    }

    pub fn get_memory(&self, memory_id: &str) -> Result<Option<MemoryEntry>> {
        self.memory_store.get(memory_id)
    }

    pub fn search_memory(&self, query: &MemoryQuery) -> Result<Vec<MemorySearchResult>> {
        self.memory_store.search(query)
    }

    pub fn forget_memory(&self, memory_id: &str) -> Result<bool> {
        self.memory_store.forget(memory_id)
    }

    pub fn supersede_memory(&self, old_memory_id: &str, new_memory_id: &str) -> Result<()> {
        self.memory_store.supersede(old_memory_id, new_memory_id)
    }
}
