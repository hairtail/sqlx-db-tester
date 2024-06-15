use sqlx::{
    migrate::{MigrationSource, Migrator},
    Connection, Executor, MySqlConnection, MySqlPool,
};
use std::{path::Path, thread};
use tokio::runtime::Runtime;
use uuid::Uuid;

#[derive(Debug)]
pub struct TestMysql {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    pub dbname: String,
}

impl TestMysql {
    pub fn new<S>(
        host: impl Into<String>,
        port: u16,
        user: impl Into<String>,
        password: impl Into<String>,
        migrations: S,
    ) -> Self
    where
        S: MigrationSource<'static> + Send + Sync + 'static,
    {
        let host = host.into();
        let user = user.into();
        let password = password.into();

        let uuid = Uuid::new_v4();
        let simple = uuid.simple();
        let dbname = format!("test_{}", simple);
        let dbname_cloned = dbname.clone();

        let tdb = Self {
            host,
            port,
            user,
            password,
            dbname,
        };

        let server_url = tdb.server_url();
        let url = tdb.url();

        // create database dbname
        thread::spawn(move || {
            let rt = Runtime::new().unwrap();
            rt.block_on(async move {
                // use server url to create database
                let mut conn = MySqlConnection::connect(&server_url).await.unwrap();
                conn.execute(format!(r#"CREATE DATABASE "{}""#, dbname_cloned).as_str())
                    .await
                    .unwrap();

                // now connect to test database for migration
                let mut conn = MySqlConnection::connect(&url).await.unwrap();
                let m = Migrator::new(migrations).await.unwrap();
                m.run(&mut conn).await.unwrap();
            });
        })
        .join()
        .expect("failed to create database");

        tdb
    }

    pub fn server_url(&self) -> String {
        if self.password.is_empty() {
            format!("mysql://{}@{}:{}", self.user, self.host, self.port)
        } else {
            format!(
                "mysql://{}:{}@{}:{}",
                self.user, self.password, self.host, self.port
            )
        }
    }

    pub fn url(&self) -> String {
        format!("{}/{}", self.server_url(), self.dbname)
    }

    pub async fn get_pool(&self) -> MySqlPool {
        MySqlPool::connect(&self.url()).await.unwrap()
    }
}

impl Drop for TestMysql {
    fn drop(&mut self) {
        let server_url = self.server_url();
        let dbname = self.dbname.clone();
        thread::spawn(move || {
            let rt = Runtime::new().unwrap();
            rt.block_on(async move {
                let mut conn = MySqlConnection::connect(&server_url).await.unwrap();
                // TODO: terminate existing connections
                conn.execute(format!(r#"DROP DATABASE "{}""#, dbname).as_str())
                    .await
                    .expect("Error while querying the drop database");
            });
        })
        .join()
        .expect("failed to drop database");
    }
}

impl Default for TestMysql {
    fn default() -> Self {
        Self::new(
            "localhost",
            5432,
            "mysql",
            "mysql",
            Path::new("./migrations"),
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::mysql::TestMysql;

    #[tokio::test]
    async fn test_mysql_should_create_and_drop() {
        let tdb = TestMysql::default();
        let pool = tdb.get_pool().await;
        // insert todo
        sqlx::query("INSERT INTO todos (title) VALUES ('test')")
            .execute(&pool)
            .await
            .unwrap();
        // get todo
        let (id, title) = sqlx::query_as::<_, (i32, String)>("SELECT id, title FROM todos")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(id, 1);
        assert_eq!(title, "test");
    }
}
