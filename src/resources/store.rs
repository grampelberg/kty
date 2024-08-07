use std::{future::ready, iter::Iterator, sync::Arc};

use futures::{FutureExt, StreamExt, TryStreamExt};
use kube::{
    runtime::{
        self,
        reflector::{self},
        watcher::Config,
        WatchStreamExt,
    },
    Api, ResourceExt,
};
use serde::de::DeserializeOwned;
use tokio::task::JoinHandle;

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
    task: JoinHandle<()>,
    reader: reflector::Store<K>,
}

impl<K> Store<K>
where
    K: kube::Resource<DynamicType = ()>
        + Clone
        + std::fmt::Debug
        + Send
        + Sync
        + DeserializeOwned
        + 'static,
{
    // TODO: need to have a way to filter stuff out (with some defaults) to keep
    // from memory going nuts.
    pub fn new(client: kube::Client) -> Self {
        let (reader, writer) = reflector::store();
        let stream = runtime::watcher(Api::<K>::all(client), Config::default())
            .map_ok(|ev| {
                ev.modify(|obj| {
                    ResourceExt::managed_fields_mut(obj).clear();
                })
            })
            .default_backoff()
            .reflect(writer)
            .applied_objects()
            .boxed();

        let task = tokio::spawn(async move {
            stream.for_each(|_| ready(())).await;
        });

        Self { task, reader }
    }

    pub fn state(&self) -> Vec<Arc<K>> {
        self.reader.state()
    }

    // TODO: the naive implementation of this (loading is false on first element of
    // the stream), happens *fast*. It feels like there should be *something* that
    // comes back when the initial sync has fully completed but I can't find
    // anything in kube-rs yet that does that.
    pub fn loading(&self) -> bool {
        false
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
        self.task.abort();
    }
}
