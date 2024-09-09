use std::{future::ready, iter::Iterator, sync::Arc};

use eyre::Result;
use futures::StreamExt;
use itertools::Itertools;
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
use crate::widget::{input, table};

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
    filter: Option<String>,
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
            filter: None,
        }
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

impl<K> table::Items for Store<K>
where
    K: Filter
        + kube::Resource<DynamicType = ()>
        + Clone
        + std::fmt::Debug
        + Send
        + Sync
        + DeserializeOwned
        + 'static,
    Arc<K>: table::Row + Compare,
{
    type Item = Arc<K>;

    fn items(&self) -> Vec<Self::Item> {
        let iter = self.reader.state().into_iter().sorted_by(Compare::cmp);

        if self.filter.is_none() {
            iter.collect()
        } else {
            iter.filter(|obj| obj.matches(self.filter.as_ref().unwrap().as_str()))
                .collect()
        }
    }
}

impl<K> input::Filterable for Store<K>
where
    K: Filter
        + kube::Resource<DynamicType = ()>
        + Clone
        + std::fmt::Debug
        + Send
        + Sync
        + DeserializeOwned
        + 'static,
{
    fn filter(&mut self) -> &mut Option<String> {
        &mut self.filter
    }
}
