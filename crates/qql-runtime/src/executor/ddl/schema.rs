use std::sync::Arc;

use crate::backend::CollectionInfo;
use crate::executor::Executor;
use qql_core::error::QqlError;
use qql_embed::TopologyNames;

impl Executor {
    /// Retrieve collection info from cache or fetch from backend and cache it.
    ///
    /// The cached value is `Arc`-shared: hits cost a refcount bump, and the
    /// fetch happens outside the lock so concurrent misses fan out to the
    /// backend instead of stacking on the mutex.
    pub(crate) async fn get_cached_collection_info(
        &self,
        collection: &str,
    ) -> Result<Arc<CollectionInfo>, QqlError> {
        {
            let guard = self.schema_cache.read().await;
            if let Some((info, _)) = guard.get(collection) {
                return Ok(info.clone());
            }
        }
        let info = Arc::new(self.client.get_collection_info(collection).await?);
        let topo = Arc::new(crate::executor::dml::query::topology_names_from_info(&info));
        {
            let mut guard = self.schema_cache.write().await;
            guard.insert(collection.to_string(), (info.clone(), topo));
        }
        Ok(info)
    }

    /// Retrieve topology names (dense/sparse/multivector) from cache or fetch from backend.
    pub(crate) async fn get_cached_topology(
        &self,
        collection: &str,
    ) -> Result<Arc<TopologyNames>, QqlError> {
        {
            let guard = self.schema_cache.read().await;
            if let Some((_, topo)) = guard.get(collection) {
                return Ok(topo.clone());
            }
        }
        let info = self.client.get_collection_info(collection).await?;
        let topo = Arc::new(crate::executor::dml::query::topology_names_from_info(&info));
        {
            let mut guard = self.schema_cache.write().await;
            guard.insert(collection.to_string(), (Arc::new(info), topo.clone()));
        }
        Ok(topo)
    }

    /// Best-effort cache peek: a hit avoids the existence probe entirely.
    /// Callers fall back to `collection_exists` + fetch on a miss, so a
    /// missing collection never pays for a schema fetch it cannot use.
    pub(crate) async fn peek_cached_collection_info(
        &self,
        collection: &str,
    ) -> Option<Arc<CollectionInfo>> {
        let guard = self.schema_cache.read().await;
        guard.get(collection).map(|(info, _)| info.clone())
    }

    /// Invalidate cached collection topology on DDL mutations.
    pub(crate) async fn invalidate_collection_schema(&self, collection: &str) {
        self.schema_cache.write().await.remove(collection);
    }
}
