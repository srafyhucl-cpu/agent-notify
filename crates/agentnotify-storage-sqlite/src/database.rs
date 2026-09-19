use std::path::Path;

use agentnotify_application::StoreError;
use rusqlite::Connection;
use tokio::sync::{mpsc, oneshot};

use crate::migrations::{configure_connection, run_migrations};

const COMMAND_QUEUE_CAPACITY: usize = 256;
const DATABASE_THREAD_NAME: &str = "agentnotify-sqlite";

type DatabaseTask = Box<dyn FnOnce(&mut Connection) + Send + 'static>;

/// 串行调度 SQLite 命令，连接只存在于专用后台线程。
#[derive(Clone)]
pub(crate) struct Database {
    sender: mpsc::Sender<DatabaseTask>,
}

impl Database {
    pub(crate) fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let mut connection = Connection::open(path)
            .map_err(|error| storage_error("打开 SQLite 数据库失败", error))?;
        configure_connection(&connection)?;
        run_migrations(&mut connection)?;
        Self::spawn(connection)
    }

    pub(crate) async fn run<T, F>(&self, operation: F) -> Result<T, StoreError>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T, StoreError> + Send + 'static,
    {
        let (response_sender, response_receiver) = oneshot::channel();
        self.sender
            .send(Box::new(move |connection| {
                let result = operation(connection);
                let _ = response_sender.send(result);
            }))
            .await
            .map_err(|_| StoreError::unavailable("SQLite 调度线程已停止"))?;

        response_receiver
            .await
            .map_err(|_| StoreError::unavailable("SQLite 操作未返回结果"))?
    }

    fn spawn(mut connection: Connection) -> Result<Self, StoreError> {
        let (sender, mut receiver): (mpsc::Sender<DatabaseTask>, mpsc::Receiver<DatabaseTask>) =
            mpsc::channel(COMMAND_QUEUE_CAPACITY);
        std::thread::Builder::new()
            .name(DATABASE_THREAD_NAME.into())
            .spawn(move || {
                while let Some(operation) = receiver.blocking_recv() {
                    operation(&mut connection);
                }
            })
            .map_err(|error| storage_error("启动 SQLite 调度线程失败", error))?;
        Ok(Self { sender })
    }
}

fn storage_error(context: &str, error: impl std::fmt::Display) -> StoreError {
    let _ = error;
    StoreError::new("sqlite_error", context)
}
