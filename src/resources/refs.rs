use eyre::Result;
use futures::{future::BoxFuture, FutureExt, TryStreamExt};
use k8s_openapi::api::core::v1::ObjectReference;
use kube::{api::ObjectMeta, Resource};
use petgraph::{graph::NodeIndex, Graph};

use super::{ApiResource, GetOwners};

pub struct References {
    client: kube::Client,
    graph: Graph<ObjectReference, ()>,
    root: NodeIndex,
}

// TODO: probably need some way to query/dedup the graph for the same node.
impl References {
    pub fn new(client: kube::Client, root: &ObjectReference) -> Self {
        let mut graph = Graph::new();
        let root = graph.add_node(root.clone());

        Self {
            client,
            graph,
            root,
        }
    }

    pub fn to(&mut self, reference: ObjectReference) -> NodeIndex {
        let idx = self.graph.add_node(reference);
        self.graph.add_edge(self.root, idx, ());

        idx
    }

    pub fn from(&mut self, reference: ObjectReference) -> NodeIndex {
        let idx = self.graph.add_node(reference);
        self.graph.add_edge(idx, self.root, ());

        idx
    }

    pub fn edge_to(&mut self, from: NodeIndex, to: ObjectReference) -> NodeIndex {
        let idx = self.graph.add_node(to);
        self.graph.add_edge(from, idx, ());

        idx
    }

    pub async fn add_owners(&mut self, meta: &ObjectMeta) -> Result<()> {
        self.idx_owners(self.root, meta).await
    }

    fn idx_owners<'a>(
        &'a mut self,
        idx: NodeIndex,
        meta: &'a ObjectMeta,
    ) -> BoxFuture<'a, Result<()>> {
        async move {
            for owner in meta
                .get_owners(self.client.clone())
                .try_collect::<Vec<_>>()
                .await?
            {
                let oi = self.graph.add_node(owner.object_ref(&owner.api_resource()));
                self.graph.add_edge(oi, idx, ());

                self.idx_owners(oi, &owner.metadata).await?;
            }

            Ok(())
        }
        .boxed()
    }

    pub fn graph(self) -> Graph<ObjectReference, ()> {
        self.graph
    }
}
