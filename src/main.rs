mod library;
mod scan;

use sqlx::SqlitePool;
use cursive::{Cursive, CursiveExt};
use cursive::views::SelectView;

#[async_std::main]
async fn main() {
    let pool = SqlitePool::connect("ereader.sqlite").await.unwrap();

    scan::scan(&pool, "epub").await.unwrap();

    let books = library::get_books(&pool).await.unwrap();

    let mut siv = Cursive::new();

    let mut view = SelectView::new().h_align(cursive::align::HAlign::Left);

    for book in &books {
        view.add_item(book.title.clone(), book.id);
    }

    siv.add_layer(cursive::views::Dialog::around(view).title("Library"));
    siv.add_global_callback('q', |s| s.quit());
    siv.run();
}
