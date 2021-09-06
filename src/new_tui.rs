use crate::library::delete_bookmark;
use crate::library::*;
use crate::Error;
use cursive::traits::*;
//use cursive::view::*;
use cursive::views::*;
use cursive::*;
use cursive_markup::html::RichRenderer;
use cursive_markup::MarkupView;
use sqlx::SqlitePool;
use std::fmt::format;
use std::future::Future;
use std::io::Write;
use tokio::runtime::Runtime;
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
    ($view:expr, button) => {
        |s| {
            match $view(s) {
                Err(e) => error_message(s, e),
                _ => {},
            }
        }
    };
    ($view:expr) => {
        |s, d| {
            match $view(s, d) {
                Err(e) => error_message(s, e),
                _ => {},
            }
        }
    };
    ($view:expr, $($args:expr),+) => {
        move |s| {
            match $view(s, $($args),+) {
                Err(e) => error_message(s, e),
                _ => {},
            }
        }
    }
}

pub fn error_message(s: &mut Cursive, e: Error) {
    s.add_layer(
        Dialog::around(TextView::new(e.to_string()))
            .dismiss_button("Close")
            .max_width(90),
    );
}

#[allow(dead_code)]
pub fn log(message: String) {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .append(true)
        .open("debug.log")
        .unwrap();

    writeln!(file, "{}", message).unwrap()
}

// ============================== LIBRARY ==============================
pub fn library(s: &mut Cursive) -> Result<(), Error> {
    let data = data(s)?;
    let books = data.run(get_books(&data.pool))?;

    let mut library = LinearLayout::vertical();

    let mut books_list = SelectView::new();
    books_list.set_on_select(set_book_details);
    books_list.set_on_submit(try_view!(|s, book: &Book| chapter_goto_index(
        s, book.id, 1
    )));

    for book in books {
        books_list.add_item(book.title.clone(), book.clone());
    }

    let book_details = TextView::new("book details").with_name("library detail");

    library.add_child(books_list);
    library.add_child(book_details);

    s.add_layer(
        Dialog::around(library.scrollable())
            .title("Library")
            .button("Bookmarks", try_view!(bookmarks, button))
            .max_width(90),
    );

    Ok(())
}

fn set_book_details(s: &mut Cursive, book: &Book) {
    let title = book.title.clone();
    s.call_on_name("library detail", move |v: &mut TextView| {
        v.set_content(title);
    });
}

// ============================== READER ==============================
fn chapter(s: &mut Cursive, id: Hyphenated, progress: Option<f32>) -> Result<(), Error> {
    let data = data(s)?;
    let chapter = data.run(get_chapter_by_id(&data.pool, id))?;
    let num_chapters = data.run(get_num_chapters(&data.pool, chapter.book_id))?;

    let cursor = std::io::Cursor::new(chapter.content.clone());
    let content = zstd::stream::decode_all(cursor).unwrap();
    let content_str = String::from_utf8(content).unwrap();

    let mut chapter_view = if let Some(c) = s.find_name::<Dialog>("reader") {
        c
    } else {
        s.add_layer(Dialog::new().with_name("reader").max_width(90));
        s.find_name::<Dialog>("reader").unwrap()
    };

    let mut view = MarkupView::html(&content_str);
    view.on_link_focus(|_s, _url| {});
    view.on_link_select(|_s, _url| {});

    let mut scrollable = view.scrollable();
    // TODO: this might still be wrong when the bookmark is near the end or at weird screen sizes
    // write out the calculations and figure out the correct way to do this
    if let Some(progress) = progress {
        let x = std::cmp::min(s.screen_size().x - 6, 86);
        scrollable.layout(XY::new(x, 65));

        let size = scrollable.inner_size();
        let offset_y = (size.y as f32 * progress).round() as usize;
        scrollable.set_offset(XY::new(0, offset_y));
    }

    chapter_view.set_content(scrollable.with_name("reader content"));

    chapter_view.clear_buttons();
    if chapter.index < num_chapters as i64 {
        let book_id = chapter.book_id;
        let index = chapter.index;
        chapter_view.add_button("Next", try_view!(chapter_goto_index, book_id, index + 1));
    }
    if chapter.index > 1 {
        let book_id = chapter.book_id;
        let index = chapter.index;
        chapter_view.add_button("Prev", try_view!(chapter_goto_index, book_id, index - 1));
    }
    let book_id = chapter.book_id;
    chapter_view.add_button("TOC", try_view!(toc, book_id));
    let book_id = chapter.book_id;
    let chapter_id = chapter.id;
    chapter_view.add_button("Bookmark", try_view!(set_bookmark, book_id, chapter_id));
    chapter_view.add_button("Close", |s| {
        s.pop_layer();
    });

    Ok(())
}

fn chapter_goto_index(s: &mut Cursive, id: Hyphenated, index: i64) -> Result<(), Error> {
    let chapter_id = {
        let data = data(s)?;
        let chapter = data.run(get_chapter(&data.pool, id, index))?;
        chapter.id
    };

    chapter(s, chapter_id, None)
}

fn chapter_goto_toc(s: &mut Cursive, toc: &Toc) -> Result<(), Error> {
    s.pop_layer();
    chapter(s, toc.chapter_id, None)
}

fn chapter_goto_bookmark(s: &mut Cursive, bookmark: &Bookmark) -> Result<(), Error> {
    s.pop_layer();
    chapter(s, bookmark.chapter_id, Some(bookmark.progress))
}

// ============================== TOC ==============================
fn toc(s: &mut Cursive, id: Hyphenated) -> Result<(), Error> {
    let data = data(s)?;
    let toc = data.run(get_toc(&data.pool, id))?;

    let mut toc_list = SelectView::new();
    for toc in toc {
        toc_list.add_item(toc.title.clone(), toc.clone());
    }

    toc_list.set_on_submit(try_view!(chapter_goto_toc));

    s.add_layer(
        Dialog::around(toc_list.scrollable())
            .title("Table of Contents")
            .dismiss_button("Close")
            .max_width(90),
    );

    Ok(())
}

// ============================== BOOKMARKS ==============================
fn bookmarks(s: &mut Cursive) -> Result<(), Error> {
    let data = data(s)?;
    let bookmarks = data.run(get_bookmarks(&data.pool))?;

    let mut bookmarks_view = SelectView::new();

    for bookmark in bookmarks {
        let book = data.run(get_book(&data.pool, bookmark.book_id))?;
        bookmarks_view.add_item(book.title.clone(), bookmark);
    }

    bookmarks_view.set_on_submit(try_view!(chapter_goto_bookmark));

    s.add_layer(
        Dialog::around(bookmarks_view.with_name("bookmarks"))
            .title("Bookmarks")
            .button("Delete", try_view!(delete_selected_bookmark, button))
            .dismiss_button("Close")
            .max_width(90),
    );

    Ok(())
}

fn delete_selected_bookmark(s: &mut Cursive) -> Result<(), Error> {
    let bookmarks_view = s.find_name::<SelectView<Bookmark>>("bookmarks").unwrap();
    let bookmark = bookmarks_view.selection().unwrap();

    log(format!("{:?}", bookmark));
    let data = data(s)?;
    data.run(delete_bookmark(&data.pool, bookmark.id))?;

    s.pop_layer();
    bookmarks(s)
}

fn set_bookmark(s: &mut Cursive, book_id: Hyphenated, chapter_id: Hyphenated) -> Result<(), Error> {
    let reader_content = s
        .find_name::<ScrollView<MarkupView<RichRenderer>>>("reader content")
        .unwrap();

    let viewport = reader_content.content_viewport();
    let size = reader_content.inner_size();
    let progress = viewport.top() as f32 / size.y as f32;

    let data = data(s)?;
    data.run(insert_bookmark(
        &data.pool,
        &Bookmark {
            id: 0,
            book_id,
            chapter_id,
            progress,
            created: chrono::Utc::now(),
        },
    ))
}
