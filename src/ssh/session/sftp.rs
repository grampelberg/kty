use std::path::Path;

use eyre::Result;
use lazy_static::lazy_static;
use prometheus::{
    opts, register_int_counter, register_int_counter_vec, register_int_gauge, IntCounter,
    IntCounterVec, IntGauge,
};
use prometheus_static_metric::make_static_metric;
use russh_sftp::{
    protocol::{self, Attrs, Data, FileAttributes, Handle, Name, OpenFlags, Status, StatusCode},
    server,
};

use crate::resources::File;

make_static_metric! {
    pub struct DirectionVec: IntCounter {
        "direction" => {
            sent,
            received,
        }
    }
}

lazy_static! {
    static ref SFTP_ACTIVE: IntGauge =
        register_int_gauge!("sftp_active_sessions", "Number of active SFTP sessions").unwrap();
    static ref SFTP_BYTES_VEC: IntCounterVec = register_int_counter_vec!(
        opts!(
            "sftp_bytes_total",
            "Number of bytes that have been transferred via SFTP",
        ),
        &["direction"],
    )
    .unwrap();
    static ref SFTP_BYTES: DirectionVec = DirectionVec::from(&SFTP_BYTES_VEC);
    static ref SFTP_FILES_VEC: IntCounterVec = register_int_counter_vec!(
        opts!(
            "sftp_files_total",
            "Number of files that have been transferred via SFTP",
        ),
        &["direction"],
    )
    .unwrap();
    static ref SFTP_FILES: DirectionVec = DirectionVec::from(&SFTP_FILES_VEC);
    static ref SFTP_STAT: IntCounter =
        register_int_counter!("sftp_stat_total", "Total stat calls via SFTP").unwrap();
    static ref SFTP_LIST: IntCounter =
        register_int_counter!("sftp_list_total", "Total list calls via SFTP").unwrap();
}

enum State {
    Unknown,
    OpenFile,
    FileComplete,
    OpenDir,
    DirComplete,
}

impl Default for State {
    fn default() -> Self {
        Self::Unknown
    }
}

pub struct Handler {
    client: kube::Client,
    state: State,
}

// TODO: would it be better to add a `Store<Pod>` to this?
impl Handler {
    pub fn new(client: kube::Client) -> Self {
        SFTP_ACTIVE.inc();

        Self {
            client,
            state: State::default(),
        }
    }
}

#[async_trait::async_trait]
impl server::Handler for Handler {
    type Error = StatusCode;

    fn unimplemented(&self) -> Self::Error {
        StatusCode::OpUnsupported
    }

    async fn open(
        &mut self,
        id: u32,
        filename: String,
        _: OpenFlags,
        _: FileAttributes,
    ) -> Result<Handle, Self::Error> {
        self.state = State::OpenFile;

        Ok(Handle {
            id,
            handle: filename,
        })
    }

    #[tracing::instrument(skip(self))]
    async fn read(
        &mut self,
        id: u32,
        handle: String,
        _offset: u64,
        _len: u32,
    ) -> Result<Data, Self::Error> {
        if !matches!(self.state, State::OpenFile) {
            return Err(StatusCode::Eof);
        }

        SFTP_FILES.sent.inc();

        self.state = State::FileComplete;

        tracing::info!("read file");

        let result = File::new(Path::new(handle.as_str()))
            .read(self.client.clone())
            .await
            .map(|data| Data { id, data })
            .map_err(|_| StatusCode::NoSuchFile);

        if let Ok(data) = &result {
            SFTP_BYTES.sent.inc_by(data.data.len() as u64);
        }

        result
    }

    async fn close(&mut self, id: u32, _handle: String) -> Result<Status, Self::Error> {
        Ok(Status {
            id,
            status_code: StatusCode::Ok,
            error_message: "Ok".to_string(),
            language_tag: "en-US".to_string(),
        })
    }

    #[tracing::instrument(skip(self))]
    async fn write(
        &mut self,
        _id: u32,
        _handle: String,
        _offset: u64,
        _data: Vec<u8>,
    ) -> Result<Status, Self::Error> {
        tracing::info!("write");

        Err(StatusCode::OpUnsupported)
    }

    async fn opendir(&mut self, id: u32, path: String) -> Result<Handle, Self::Error> {
        self.state = State::OpenDir;

        Ok(Handle { id, handle: path })
    }

    #[tracing::instrument(skip(self))]
    async fn readdir(&mut self, id: u32, handle: String) -> Result<Name, Self::Error> {
        SFTP_LIST.inc();
        tracing::info!("readdir");

        if !matches!(self.state, State::OpenDir) {
            return Err(StatusCode::Eof);
        }

        self.state = State::DirComplete;

        let path = Path::new(handle.as_str());

        File::new(path)
            .list(self.client.clone())
            .await
            .map(|files| Name { id, files })
            .map_err(|e| {
                tracing::error!("readdir: {:?}", e);
                StatusCode::NoSuchFile
            })
    }

    async fn realpath(&mut self, id: u32, _: String) -> Result<Name, Self::Error> {
        Ok(Name {
            id,
            files: vec![protocol::File {
                filename: String::new(),
                longname: String::new(),
                attrs: FileAttributes::default(),
            }],
        })
    }

    async fn stat(&mut self, id: u32, path: String) -> Result<Attrs, Self::Error> {
        SFTP_STAT.inc();
        tracing::info!("stat: {}", path);

        File::new(Path::new(path.as_str()))
            .stat(self.client.clone())
            .await
            .map(|attrs| Attrs { id, attrs })
            .map_err(|e| {
                tracing::error!("stat: {:?}", e);
                StatusCode::NoSuchFile
            })
    }

    async fn lstat(&mut self, id: u32, path: String) -> Result<Attrs, Self::Error> {
        self.stat(id, path).await
    }

    #[tracing::instrument(skip(self))]
    async fn fstat(&mut self, id: u32, path: String) -> Result<Attrs, Self::Error> {
        self.stat(id, path).await
    }
}

impl Drop for Handler {
    fn drop(&mut self) {
        SFTP_ACTIVE.dec();
    }
}
