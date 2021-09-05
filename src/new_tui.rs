use crate::library::*;
use crate::Error;
use cursive::traits::*;
use cursive::view::*;
use cursive::views::*;
use cursive::*;
use sqlx::SqlitePool;
use uuid::Uuid;
use uuid::adapter::Hyphenated;

struct UserData {
    pool: SqlitePool,
}

pub async fn library(s: &mut Cursive, pool: &SqlitePool) -> Result<(), Error> {
    let books = get_books(pool).await?;

    let mut library = LinearLayout::vertical();

    let mut books_list = SelectView::new();

    for book in books {
        books_list.add_item(book.title.clone(), book.clone());
    }
    books_list.set_on_select(set_book_details);

    let mut book_details = TextView::new("book details")
        .with_name("details");


    library.add_child(books_list);
    library.add_child(book_details);

    s.add_layer(Dialog::around(library).title("Library"));

    Ok(())
}

fn set_book_details(s: &mut Cursive, book: &Book) {
    let title = book.title.clone();
    s.call_on_name("details", move |v: &mut TextView| {
        v.set_content(title);
    });
}

