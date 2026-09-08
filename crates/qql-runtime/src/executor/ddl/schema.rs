use std::sync::Arc;

use crate::backend::CollectionInfo;
use crate::executor::Executor;
use qql_core::error::QqlError;
use qql_embed::TopologyNames;

impl Executor {
    /// Retrieve collection info from cache or fetch from backend and cache it.
    pub(crate) async fn get_cached_collection_info(
        &self,
        collection: &str,
    ) -> Result<CollectionInfo, QqlError> {
        if let Ok(guard) = self.schema_cache.read()
            && let Some((info, _)) = guard.get(collection)
        {
            return Ok(info.clone());
        }
        let info = self.client.get_collection_info(collection).await?;
        let topo = Arc::new(crate::executor::dml::query::topology_names_from_info(&info));
        if let Ok(mut guard) = self.schema_cache.write() {
            guard.insert(collection.to_string(), (info.clone(), topo));
        }
        Ok(info)
    }

    /// Retrieve topology names (dense/sparse/multivector) from cache or fetch from backend.
    pub(crate) async fn get_cached_topology(
        &self,
        collection: &str,
    ) -> Result<Arc<TopologyNames>, QqlError> {
        if let Ok(guard) = self.schema_cache.read()
            && let Some((_, topo)) = guard.get(collection)
        {
            return Ok(topo.clone());
        }
        let info = self.client.get_collection_info(collection).await?;
        let topo = Arc::new(crate::executor::dml::query::topology_names_from_info(&info));
        if let Ok(mut guard) = self.schema_cache.write() {
            guard.insert(collection.to_string(), (info, topo.clone()));
        }
        Ok(topo)
    }

    /// Invalidate cached collection topology on DDL mutations.
    pub(crate) fn invalidate_collection_schema(&self, collection: &str) {
        if let Ok(mut guard) = self.schema_cache.write() {
            guard.remove(collection);
        }
    }
}
