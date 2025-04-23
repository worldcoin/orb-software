use sqlx::{self};

// This test spins up a brand‑new temporary DB, applies the migrations (handled
// automatically by the `#[sqlx::test]` macro) and verifies we can run a simple
// query.

#[sqlx::test]
async fn can_select_one(pool: sqlx::PgPool) -> sqlx::Result<()> {
    let row: (i32,) = sqlx::query_as("SELECT 1")
        .fetch_one(&pool)
        .await?;

    assert_eq!(row.0, 1);
    Ok(())
}
