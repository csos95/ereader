mod library;
mod scan;

use async_std::task;
use cursive::traits::Scrollable;
use cursive::view::{Nameable, Resizable};
use cursive::views::{Dialog, SelectView, TextView};
use cursive::{Cursive, CursiveExt, View, XY};
use library::{Book, Bookmark, Chapter, Toc};
use sqlx::SqlitePool;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("sqlx error {0}")]
    SqlxError(sqlx::Error),
    #[error("unable to parse epub")]
    UnableToParseEpub,
    #[error("{0} is missing metadata tag {1}")]
    MissingMetadata(String, String),
    #[error("unable to get resource")]
    UnableToGetResource,
    #[error("invalid spine index: {0}")]
    InvalidSpineIndex(usize),
    #[error("anyhow error {0}")]
    AnyhowError(anyhow::Error),
    #[error("unable to parse html")]
    UnableToParseHTML,
    #[error("unable to find {0} in html")]
    UnableToFindSelector(String),
    #[error("io error {0}")]
    IOError(std::io::Error),
    #[error("debug message {0}")]
    DebugMsg(String),
}

impl From<sqlx::Error> for Error {
    fn from(e: sqlx::Error) -> Self {
        Error::SqlxError(e)
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::IOError(e)
    }
}

impl From<anyhow::Error> for Error {
    fn from(e: anyhow::Error) -> Self {
        Error::AnyhowError(e)
    }
}

#[derive(Clone, Debug)]
struct Model {
    pool: SqlitePool,
    page: Page,
}

#[derive(Clone, Debug)]
enum Page {
    Library(Vec<Book>),
    Chapter(Chapter, Option<f32>),
    TableOfContents(Vec<Toc>, i64),
    Bookmarks(Vec<Bookmark>),
}

enum Msg {
    GoLibrary,
    GoChapterIndex(i64, i64),
    GoChapterId(i64),
    GoChapterIdBookmark(i64, f32),
    NextChapter,
    PrevChapter,
    GoTOC,
    Scan,
    GoBookmarks,
    SetBookmark(i64, i64, f32),
}

async fn init() -> Result<Model, Error> {
    let pool = SqlitePool::connect("ereader.sqlite").await?;

    let books = library::get_books(&pool).await?;

    Ok(Model {
        pool,
        page: Page::Library(books),
    })
}

fn update_view(s: &mut Cursive, msg: Msg) {
    let model: Model = s.take_user_data().unwrap();

    match update(msg, model.clone()) {
        Ok(model) => {
            s.pop_layer();
            view(s, &model);
        
            s.set_user_data(model);
        },
        Err(e) => {
            error(s, e);
            s.set_user_data(model);
        }
    }
}

fn update(msg: Msg, mut model: Model) -> Result<Model, Error> {
    let pool = &model.pool;
    model.page = match (msg, model.page) {
        (Msg::GoLibrary, _) => {
            let books = task::block_on(async { library::get_books(pool).await })?;
            Page::Library(books)
        }
        (Msg::GoChapterIndex(book_id, index), _) => {
            let chapter =
                task::block_on(async { library::get_chapter(pool, book_id, index).await })?;
            Page::Chapter(chapter, None)
        }
        (Msg::NextChapter, Page::Chapter(chapter, _)) => {
            let chapter = task::block_on(async {
                library::get_chapter(pool, chapter.book_id, chapter.index + 1).await
            })?;
            Page::Chapter(chapter, None)
        }
        (Msg::PrevChapter, Page::Chapter(chapter, _)) => {
            let chapter = task::block_on(async {
                library::get_chapter(pool, chapter.book_id, chapter.index - 1).await
            })?;
            Page::Chapter(chapter, None)
        }
        (Msg::GoTOC, Page::Chapter(chapter, _)) => {
            let toc = task::block_on(async { library::get_toc(pool, chapter.book_id).await })?;
            Page::TableOfContents(toc, chapter.book_id)
        }
        (Msg::GoChapterId(id), _) => {
            let chapter = task::block_on(async { library::get_chapter_by_id(pool, id).await })?;
            Page::Chapter(chapter, None)
        }
        // Separate cases for library/other page so that scanning can be done at any time
        // and not necessarily tied to the library page
        (Msg::Scan, Page::Library(_)) => {
            let books = task::block_on(async {
                scan::scan(pool, "epub").await?;
                library::get_books(pool).await
            })?;
            Page::Library(books)
        }
        (Msg::Scan, page) => {
            task::block_on(async {
                scan::scan(pool, "epub").await
            })?;
            page
        }
        (Msg::GoBookmarks, _) => {
            let bookmarks = task::block_on(async {
                library::get_bookmarks(pool).await
            })?;
            Page::Bookmarks(bookmarks)
        }
        (Msg::SetBookmark(book_id, chapter_id, progress), Page::Chapter(chapter, _)) => {
            task::block_on(async {
                library::insert_bookmark(pool, &library::Bookmark {
                    id: 0,
                    book_id,
                    chapter_id,
                    progress,
                    created: chrono::Utc::now(),
                }).await
            })?;
            Page::Chapter(chapter, Some(progress))
        }
        (Msg::GoChapterIdBookmark(id, progress), _) => {
            let chapter = task::block_on(async { library::get_chapter_by_id(pool, id).await })?;
            Page::Chapter(chapter, Some(progress))
        }
        (_msg, page) => page,
    };

    Ok(model)
}

fn view(s: &mut Cursive, model: &Model) {
    match &model.page {
        Page::Chapter(chapter, progress) => view_chapter(s, chapter, *progress),
        Page::Library(books) => view_library(s, books),
        Page::TableOfContents(toc, book_id) => view_toc(s, toc, *book_id),
        Page::Bookmarks(bookmarks) => view_bookmarks(s, bookmarks),
    }
}

#[async_std::main]
async fn main() {
    let mut siv = Cursive::new();

    let model = init().await.unwrap();
    view(&mut siv, &model);
    siv.set_user_data(model);

    siv.add_global_callback('q', |s| s.quit());
    siv.add_global_callback('l', |s| {
        s.cb_sink()
            .send(Box::new(move |s| update_view(s, Msg::GoLibrary)))
            .unwrap();
    });
    siv.run();
}

fn error(s: &mut Cursive, e: Error) {
    s.add_layer(
        Dialog::around(TextView::new(format!("{:?}", e)))
            .title("Error")
            .button("Close", |s| {
                s.pop_layer();
            })
            .max_width(90),
    );
}

fn view_library(s: &mut Cursive, books: &[Book]) {
    let mut view = SelectView::new();

    for book in books {
        view.add_item(book.title.clone(), book.id);
    }

    view.set_on_submit(|s: &mut Cursive, id: &i64| {
        let b_id = *id;
        s.cb_sink()
            .send(Box::new(move |s| {
                update_view(s, Msg::GoChapterIndex(b_id, 1))
            }))
            .unwrap();
    });

    s.add_layer(
        Dialog::around(view.scrollable())
            .title("Library")
            .button("Bookmarks", |s| update_view(s, Msg::GoBookmarks))
            .button("Scan", |s| update_view(s, Msg::Scan))
            .max_width(90),
    );
}

fn view_chapter(s: &mut Cursive, chapter: &Chapter, progress: Option<f32>) {
    let cursor = std::io::Cursor::new(chapter.content.clone());
    let content = zstd::stream::decode_all(cursor).unwrap();
    let content_str = String::from_utf8(content).unwrap();
    let mut view = cursive_markup::MarkupView::html(&content_str[..]);
    view.on_link_focus(|_s, _url| {});
    view.on_link_select(|_s, _url| {});

    let mut scrollable = view.scrollable();
    if let Some(progress) = progress {
        scrollable.layout(XY::new(84, 65));
    
        let size = scrollable.inner_size();
        let offset_y = (size.y as f32 * progress).round() as usize;
        scrollable.set_offset(cursive::XY::new(0, offset_y));
    }

    let mut dialog = Dialog::around(scrollable.with_name("reader"));

    // if chapter.index + 1 < chapter.epub.get_num_pages() {
    dialog.add_button("Next", move |s| {
        s.cb_sink()
            .send(Box::new(move |s| update_view(s, Msg::NextChapter)))
            .unwrap();
    });
    // }

    if chapter.index > 1 {
        dialog.add_button("Prev", move |s| {
            s.cb_sink()
                .send(Box::new(move |s| update_view(s, Msg::PrevChapter)))
                .unwrap();
        });
    }

    dialog.add_button("TOC", move |s| {
        s.cb_sink()
            .send(Box::new(move |s| update_view(s, Msg::GoTOC)))
            .unwrap();
    });

    let b_id = chapter.book_id;
    let c_id = chapter.id;
    dialog.add_button("Bookmark", move |s| {
        let (viewport, size) = s.call_on_name("reader", |view: &mut cursive::views::ScrollView<cursive_markup::MarkupView<cursive_markup::html::RichRenderer>>| {
            (view.content_viewport(), view.inner_size())
        }).unwrap();
        let progress = viewport.top() as f32 / size.y as f32;
        s.cb_sink()
            .send(Box::new(move |s| update_view(s, Msg::SetBookmark(b_id, c_id, progress))))
            .unwrap();
    });

    s.add_layer(dialog.max_width(90));
}

fn view_toc(s: &mut Cursive, toc: &[Toc], book_id: i64) {
    let mut view = SelectView::new();

    for toc in toc {
        view.add_item(toc.title.clone(), toc.chapter_id);
    }

    if toc.is_empty() {
        view.add_item("No table of contents. Go to start.".to_string(), 0);
    }

    view.set_on_submit(move |s, id| {
        let c_id = *id;
        s.cb_sink()
            .send(Box::new(move |s| if c_id == 0 {
                update_view(s, Msg::GoChapterIndex(book_id, 1));
            } else {
                update_view(s, Msg::GoChapterId(c_id));
            }))
            .unwrap();
    });

    s.add_layer(
        Dialog::around(view.scrollable())
            .title("Table of Contents")
            .max_width(90),
    );
}

fn view_bookmarks(s: &mut Cursive, bookmarks: &[Bookmark]) {
    let mut view = SelectView::new();

    for bookmark in bookmarks {
        view.add_item(bookmark.created.to_string(), (bookmark.chapter_id, bookmark.progress));
    }

    if bookmarks.is_empty() {
        view.add_item("No bookmarks. Go to library.".to_string(), (0, 0.0));
    }

    view.set_on_submit(move |s, (id, progress)| {
        let c_id = *id;
        let c_progress = *progress;
        s.cb_sink()
            .send(Box::new(move |s| if c_id == 0 {
                update_view(s, Msg::GoLibrary);
            } else {
                update_view(s, Msg::GoChapterIdBookmark(c_id, c_progress));
            }))
            .unwrap();
    });

    s.add_layer(
        Dialog::around(view.scrollable())
            .title("Bookmarks")
            .max_width(90),
    );
}
