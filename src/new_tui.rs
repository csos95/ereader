use crate::library::*;
use crate::Error;
use cursive::traits::*;
use cursive::view::*;
use cursive::views::*;
use cursive::*;
use cursive_markup::MarkupView;
use sqlx::SqlitePool;
use std::future::Future;
use tokio::runtime::Runtime;
use uuid::Uuid;
use uuid::adapter::Hyphenated;

pub struct Data {
    pub pool: SqlitePool,
    pub runtime: Runtime,
}

impl Data {
    pub fn run<F: Future>(&self, f: F) -> F::Output {
        self.runtime.block_on(f)
    }
}

pub async fn init() -> Result<Data, Error> {
    Ok(Data {
        pool: SqlitePool::connect("ereader.sqlite").await?,
        runtime: Runtime::new()?,
    })
}

fn data(s: &mut Cursive) -> Result<&mut Data, Error> {
    s.user_data().ok_or(Error::MissingUserData)
}

macro_rules! try_view {
    ($view:expr) => {
        |s, d| {
            match $view(s, d) {
                Ok(()) => {},
                Err(e) => error_message(s, e),
            }
        }
    }
}

pub fn error_message(s: &mut Cursive, e: Error) {
    s.add_layer(
        Dialog::around(TextView::new(e.to_string()))
            .dismiss_button("Close")
    );
}

pub fn library(s: &mut Cursive) -> Result<(), Error> {
    let data = data(s)?;
    let books = data.run(get_books(&data.pool))?;

    let mut library = LinearLayout::vertical();

    let mut books_list = SelectView::new();
    books_list.set_on_select(set_book_details);
    books_list.set_on_submit(try_view!(chapter));

    for book in books {
        books_list.add_item(book.title.clone(), book.clone());
    }

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

fn chapter(s: &mut Cursive, book: &Book) -> Result<(), Error> {
    s.add_layer(Dialog::new().with_name("chapter"));
    set_chapter(book.id, 1)(s);

    Ok(())
}

fn set_chapter(id: Hyphenated, index: i64) -> impl Fn(&mut Cursive) {
    move |s| {
        let data = data(s).unwrap();
        let chapter = data.run(get_chapter(&data.pool, id, index)).unwrap();
        let num_chapters = data.run(get_num_chapters(&data.pool, id)).unwrap();

        let cursor = std::io::Cursor::new(chapter.content.clone());
        let content = zstd::stream::decode_all(cursor).unwrap();
        let content_str = String::from_utf8(content).unwrap();

        let mut chapter = s.find_name::<Dialog>("chapter").unwrap();

        let mut view = MarkupView::html(&content_str);
        view.on_link_focus(|_s, _url| {});
        view.on_link_select(|_s, _url| {});

        chapter.set_content(view.scrollable());

        chapter.clear_buttons();
        if index < num_chapters as i64 {
            chapter.add_button("Next", set_chapter(id, index+1));
        }
        if index > 1 {
            chapter.add_button("Prev", set_chapter(id, index-1));
        }
        chapter.add_button("Close", |s| { s.pop_layer(); });
    }
}

