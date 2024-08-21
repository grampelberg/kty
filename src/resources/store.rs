use std::{future::ready, iter::Iterator, sync::Arc};

use eyre::Result;
use futures::StreamExt;
use kube::{
    runtime::{
        self,
        reflector::{self, store::WriterDropped},
        watcher::Config,
        WatchStreamExt,
    },
    Api, ResourceExt,
};
use serde::de::DeserializeOwned;
use tokio::task::JoinHandle;

use super::{Compare, Filter};
use crate::widget::{table, TableRow};

async fn is_ready<K>(reader: reflector::Store<K>) -> Result<(), WriterDropped>
where
    K: kube::Resource<DynamicType = ()>
        + Clone
        + std::fmt::Debug
        + Send
        + Sync
        + DeserializeOwned
        + 'static,
{
    reader.wait_until_ready().await
}

pub struct Store<K>
where
    K: kube::Resource<DynamicType = ()>
        + Clone
        + std::fmt::Debug
        + Send
        + Sync
        + DeserializeOwned
        + 'static,
{
    is_ready: JoinHandle<Result<(), WriterDropped>>,
    watcher: JoinHandle<()>,
    reader: reflector::Store<K>,
}

impl<K> Store<K>
where
    K: Filter
        + kube::Resource<DynamicType = ()>
        + Clone
        + std::fmt::Debug
        + Send
        + Sync
        + DeserializeOwned
        + 'static,
    Arc<K>: Compare,
{
    // TODO: need to have a way to filter stuff out (with some defaults) to keep
    // from memory going nuts.
    pub fn new(client: kube::Client) -> Self {
        let (reader, writer) = reflector::store();
        let stream = runtime::watcher(Api::<K>::all(client), Config::default())
            .default_backoff()
            .modify(|obj| {
                ResourceExt::managed_fields_mut(obj).clear();
            })
            .reflect(writer)
            .applied_objects()
            .boxed();

        let watcher = tokio::spawn(async move {
            stream.for_each(|_| ready(())).await;
        });

        let is_ready = tokio::spawn(is_ready(reader.clone()));

        Self {
            is_ready,
            watcher,
            reader,
        }
    }

    pub fn items(&self, filter: Option<String>) -> Vec<Arc<K>> {
        let mut items = filter
            .map(|filter| {
                self.reader
                    .state()
                    .into_iter()
                    .filter(|obj| obj.matches(filter.as_str()))
                    .collect()
            })
            .unwrap_or(self.reader.state());

        items.sort_by(Compare::cmp);

        items
    }

    pub fn get(&self, idx: usize, filter: Option<String>) -> Option<Arc<K>> {
        self.items(filter).get(idx).cloned()
    }

    pub fn loading(&self) -> bool {
        !self.is_ready.is_finished()
    }
}

impl<K> Drop for Store<K>
where
    K: kube::Resource<DynamicType = ()>
        + Clone
        + std::fmt::Debug
        + Send
        + Sync
        + DeserializeOwned
        + 'static,
{
    fn drop(&mut self) {
        self.watcher.abort();
        self.is_ready.abort();
    }
}

impl<'a, K> table::Content<'a, Arc<K>> for Arc<Store<K>>
where
    K: Filter
        + kube::Resource<DynamicType = ()>
        + Clone
        + std::fmt::Debug
        + Send
        + Sync
        + DeserializeOwned
        + 'static,
    Arc<K>: TableRow<'a> + Compare,
{
    fn items(&self, filter: Option<String>) -> Vec<impl TableRow<'a>> {
        Store::items(self, filter)
    }
}
