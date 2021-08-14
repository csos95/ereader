mod library;
mod scan;

use sqlx::SqlitePool;

#[async_std::main]
async fn main() {
    let pool = SqlitePool::connect("test.sqlite").await.unwrap();

    scan::scan(&pool, "epub").await.unwrap();
}
