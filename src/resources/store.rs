use std::{future::ready, iter::Iterator, sync::Arc};

use eyre::{eyre, Result};
use futures::StreamExt;
use kube::{
    runtime::{self, reflector, watcher::Config, WatchStreamExt},
    Api, ResourceExt,
};
use serde::de::DeserializeOwned;
use tokio::{sync::oneshot, task::JoinSet};

use super::{Compare, Filter};
use crate::widget::table;

async fn is_ready<K>(reader: reflector::Store<K>, tx: oneshot::Sender<()>) -> Result<()>
where
    K: kube::Resource<DynamicType = ()>
        + Clone
        + std::fmt::Debug
        + Send
        + Sync
        + DeserializeOwned
        + 'static,
{
    reader.wait_until_ready().await?;

    tx.send(()).map_err(|()| eyre!("receiver dropped"))?;

    Ok(())
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
    tasks: JoinSet<Result<()>>,
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
    pub fn new(client: kube::Client) -> (Arc<Self>, oneshot::Receiver<()>) {
        let (reader, writer) = reflector::store();
        let stream = runtime::watcher(Api::<K>::all(client), Config::default())
            .default_backoff()
            .modify(|obj| {
                ResourceExt::managed_fields_mut(obj).clear();
            })
            .reflect(writer)
            .applied_objects()
            .boxed();

        let mut tasks = JoinSet::new();

        tasks.spawn(async move {
            stream.for_each(|_| ready(())).await;

            Ok(())
        });

        let (tx, rx) = oneshot::channel();
        tasks.spawn(is_ready(reader.clone(), tx));

        (Arc::new(Self { tasks, reader }), rx)
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
        self.tasks.abort_all();
    }
}

impl<K> table::Items for Arc<Store<K>>
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

    fn items(&self, filter: Option<String>) -> Vec<Self::Item> {
        Store::items(self, filter)
    }
}
