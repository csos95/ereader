#![allow(dead_code)]

mod library;
mod scan;

use async_std::task;
use cursive::traits::Scrollable;
use cursive::view::{Nameable, Resizable};
use cursive::views::{Dialog, ScrollView, SelectView, TextView};
use cursive::{Cursive, CursiveExt, View, XY};
use cursive_markup::html::RichRenderer;
use cursive_markup::MarkupView;
use library::{Book, Bookmark, Chapter, Toc};
use sqlx::SqlitePool;
use std::io::Write;
use thiserror::Error;
use uuid::adapter::Hyphenated;
use uuid::Uuid;

#[derive(Error, Debug)]
pub enum Error {
    #[error("sqlx error {0}")]
    SqlxError(sqlx::Error),
    #[error("unable to parse epub")]
    UnableToParseEpub,
    #[error("missing metadata tag {0}")]
    MissingMetadata(String),
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
    #[error("url parse error {0}")]
    UrlParseError(url::ParseError),
    #[error("epub missing resource listed in table of contents")]
    EpubMissingTocResource,
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

impl From<url::ParseError> for Error {
    fn from(e: url::ParseError) -> Self {
        Error::UrlParseError(e)
    }
}

use std::fs::File;
use std::path::Path;
use std::io::{Lines, BufReader, BufRead};

type FileLines = Lines<BufReader<File>>;

fn file_lines<P: AsRef<Path>>(path: P) -> Result<FileLines, Error> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    Ok(reader.lines())
}

use serde::Deserialize;

#[derive(Deserialize, Debug)]
struct FimfArchiveAuthor {
    id: i64,
    name: String,
    #[serde(rename = "bio.html")]
    bio: Option<String>,
}

#[derive(Deserialize, Debug)]
struct FimfArchiveTag {
    id: i64,
    name: String,
    #[serde(rename = "type")]
    category: String,
}

#[derive(Deserialize, Debug)]
struct FimfArchiveArchive {
    path: String,
}

#[derive(Deserialize, Debug)]
struct FimfArchiveBook {
    id: i64,
    archive: FimfArchiveArchive,
    author: FimfArchiveAuthor,
    title: Option<String>,
    #[serde(rename = "description_html")]
    description: Option<String>,
    #[serde(rename = "completion_status")]
    status: String,
    #[serde(rename = "content_rating")]
    rating: String,
    #[serde(rename = "num_likes")]
    likes: i64,
    #[serde(rename = "num_dislikes")]
    dislikes: i64,
    #[serde(rename = "num_words")]
    words: i64,
    tags: Vec<FimfArchiveTag>,
}

fn wilson_bounds(positive: f64, negative: f64) -> (f64, f64) {
    let total = positive + negative;

    let phat = positive / total;
    let z = 1.96;

    let a = phat + z * z / (2.0 * total);
    let b = z * ((phat * (1.0 - phat) + z * z / (4.0 * total)) / total).sqrt();
    let c = 1.0 + z * z / total;

    ((a - b) / c, (a + b) / c)
}

use tantivy::collector::TopDocs;
use tantivy::query::{Occur, TermQuery, BooleanQuery, QueryParser, Query};
use tantivy::schema::*;
use tantivy::Index;
use tantivy::ReloadPolicy;
use tantivy::IndexReader;
use regex::Regex;
use regex::Captures;

fn search(mut input: String, limit: usize, index: &Index, schema: &FimfArchiveSchema, reader: &IndexReader) {
    let searcher = reader.searcher();

    let mut queries: Vec<(Occur, Box<dyn Query>)> = Vec::new();

    // used by author and tag
    let paren_escape_re = Regex::new(r#"\\\)"#).unwrap();

    // ===================== AUTHOR =======================
    let author_re = Regex::new(r#"author\(((?:\\\)|[^\)])+)\)"#).unwrap();
    let mut authors = Vec::new();

    input = author_re.replace_all(&input, |caps: &Captures| {
        let name = paren_escape_re.replace_all(&caps[1], |caps: &Captures| {
            caps[1].to_string()
        });
        authors.push(name.to_string());
        String::new()
    }).to_string();

    if authors.len() == 1 {
        let facet = Facet::from_path(&["author", &authors[0]]);
        println!("{}", facet);
        let term = Term::from_facet(schema.author, &facet);
        let query = TermQuery::new(term, IndexRecordOption::Basic);
        queries.push((Occur::Must, Box::new(query)));
    } else if authors.len() > 1 {
        let mut author_queries: Vec<(Occur, Box<dyn Query>)> = Vec::new();

        for author in authors {
            let facet = Facet::from_path(&["author", &author]);
            println!("{}", facet);
            let term = Term::from_facet(schema.author, &facet);
            let query = TermQuery::new(term, IndexRecordOption::Basic);
            author_queries.push((Occur::Should, Box::new(query)));
        }

        queries.push((Occur::Must, Box::new(BooleanQuery::new(author_queries))));
    }

    // ===================== TAG ==========================
    let mut all_tag_queries: Vec<(Occur, Box<dyn Query>)> = Vec::new();
    // This first block is for excluded tags
    let ex_tag_re = Regex::new(r#"-#\(((?:\\\)|[^\)])+)\)"#).unwrap();
    let mut ex_tags = Vec::new();

    input = ex_tag_re.replace_all(&input, |caps: &Captures| {
        let name = paren_escape_re.replace_all(&caps[1], |caps: &Captures| caps[1].to_string());
        ex_tags.push(name.to_string());
        String::new()
    }).to_string();

    if ex_tags.len() != 0 {
        for ex_tag in ex_tags {
            let facet = Facet::from_path(&["tag", &ex_tag]);
            println!("ex {}", facet);
            let term = Term::from_facet(schema.tag, &facet);
            let query = TermQuery::new(term, IndexRecordOption::Basic);
            //ex_tag_queries.push((Occur::MustNot, Box::new(query)));
            all_tag_queries.push((Occur::MustNot, Box::new(query)));
        }
    }

    // This second block is for "or" tags (at least one of them must be present)
    let or_tag_re = Regex::new(r#"~#\(((?:\\\)|[^\)])+)\)"#).unwrap();
    let mut or_tags = Vec::new();

    input = or_tag_re.replace_all(&input, |caps: &Captures| {
        let name = paren_escape_re.replace_all(&caps[1], |caps: &Captures| caps[1].to_string());
        or_tags.push(name.to_string());
        String::new()
    }).to_string();

    if or_tags.len() != 0 {
        let mut or_tag_queries: Vec<(Occur, Box<dyn Query>)> = Vec::new();

        for or_tag in or_tags {
            let facet = Facet::from_path(&["tag", &or_tag]);
            println!("or {}", facet);
            let term = Term::from_facet(schema.tag, &facet);
            let query = TermQuery::new(term, IndexRecordOption::Basic);
            or_tag_queries.push((Occur::Should, Box::new(query)));
            //all_tag_queries.push((Occur::Should, Box::new(query)));
        }

        all_tag_queries.push((Occur::Must, Box::new(BooleanQuery::new(or_tag_queries))));
    }

    // This second block is for required tags
    let tag_re = Regex::new(r#"#\(((?:\\\)|[^\)])+)\)"#).unwrap();
    let mut tags = Vec::new();

    input = tag_re.replace_all(&input, |caps: &Captures| {
        let name = paren_escape_re.replace_all(&caps[1], |caps: &Captures| caps[1].to_string());
        tags.push(name.to_string());
        String::new()
    }).to_string();

    if tags.len() != 0 {
        for tag in tags {
            let facet = Facet::from_path(&["tag", &tag]);
            println!("{}", facet);
            let term = Term::from_facet(schema.tag, &facet);
            let query = TermQuery::new(term, IndexRecordOption::Basic);
            //tag_queries.push((Occur::Must, Box::new(query)));
            all_tag_queries.push((Occur::Must, Box::new(query)));
        }
    }

    // put the excluded and required tags together into one query
    if all_tag_queries.len() != 0 {
        queries.push((Occur::Must, Box::new(BooleanQuery::new(all_tag_queries))));
    }
    // ===================== WORDS ========================
    // ===================== LIKES ========================
    // ===================== DISLIKES =====================
    // ===================== WILSON =======================
    input = input.trim_start().trim_end().to_string();
    println!("input: [{}]", input);
    if input.len() != 0 {
        let query_parser = QueryParser::for_index(&index, vec![schema.title, schema.description]);
        let text_query = query_parser.parse_query(&input).unwrap();
    
        queries.push((Occur::Must, Box::new(text_query)));
    }

    let query = BooleanQuery::new(queries);
    println!("{:?}", query);

    let top_docs: Vec<(f32, tantivy::DocAddress)> = searcher.search(&query, &TopDocs::with_limit(limit)).unwrap();

    println!("There are {} results.", top_docs.len());
    for (score, doc_address) in top_docs {
        let retrieved_doc = searcher.doc(doc_address).unwrap();
        //println!("{} {}", score, schema.schema.to_json(&retrieved_doc));
        println!(
            "{:?} by {:?} tags {:?}",
            retrieved_doc.get_first(schema.title).unwrap().text().unwrap(),
            retrieved_doc.get_first(schema.author).unwrap().path().unwrap(),
            retrieved_doc.get_all(schema.tag).map(|f| f.path().unwrap()).collect::<Vec<String>>(),
        );
    }

}

struct FimfArchiveSchema {
    schema: Schema,
    title: Field,
    description: Field,
    author: Field,
    path: Field,
    likes: Field,
    dislikes: Field,
    words: Field,
    wilson: Field,
    status: Field,
    rating: Field,
    tag: Field,
}

impl FimfArchiveSchema {

    fn new() -> Self {
        let mut schema_builder = Schema::builder();
        schema_builder.add_text_field("title", TEXT | STORED);
        schema_builder.add_text_field("description", TEXT | STORED);
        schema_builder.add_facet_field("author", INDEXED | STORED);
        schema_builder.add_text_field("path", TEXT | STORED);
        schema_builder.add_i64_field("likes", INDEXED | STORED | FAST);
        schema_builder.add_i64_field("dislikes", INDEXED | STORED | FAST);
        schema_builder.add_i64_field("words", INDEXED | STORED | FAST);
        schema_builder.add_f64_field("wilson", INDEXED | STORED | FAST);
        schema_builder.add_facet_field("status", INDEXED | STORED);
        schema_builder.add_facet_field("rating", INDEXED | STORED);
        schema_builder.add_facet_field("tag", INDEXED | STORED);
        let schema = schema_builder.build();

        FimfArchiveSchema {
            schema: schema.clone(),
            title: schema.get_field("title").unwrap(),
            description: schema.get_field("description").unwrap(),
            author: schema.get_field("author").unwrap(),
            path: schema.get_field("path").unwrap(),
            likes: schema.get_field("likes").unwrap(),
            dislikes: schema.get_field("dislikes").unwrap(),
            words: schema.get_field("words").unwrap(),
            wilson: schema.get_field("wilson").unwrap(),
            status: schema.get_field("status").unwrap(),
            rating: schema.get_field("rating").unwrap(),
            tag: schema.get_field("tag").unwrap(),
        }
    }
}

fn import_fimfarchive<P: AsRef<Path>>(path: P, index: &Index, schema: &FimfArchiveSchema, limit: usize) -> Result<(), Error> {
    let mut index_writer = index.writer(16_000_000).unwrap();

    for (i, line) in file_lines(path).unwrap().take(limit).enumerate() {
        let line = line.unwrap();
        if line.len() != 1 {
            // ignore the object key and trailing comma
            let mut start = 0;
            for j in 0..line.len() {
                if line.as_bytes()[j] == b'{' {
                    start = j;
                    break;
                }
            }
            let end = if line.as_bytes()[line.len()-1] == b'}' {
                line.len()
            } else {
                line.len() - 1
            };
            let object = &line[start..end];

            let book: FimfArchiveBook = serde_json::from_str(object).unwrap();

            let mut doc = Document::default();
            if let Some(t) = book.title {
                doc.add_text(schema.title, t);
            } else {
                doc.add_text(schema.title, "UNTITLED");
            }
            if let Some(d) = book.description {
                doc.add_text(schema.description, d);
            } else {
                doc.add_text(schema.description, "");
            }

            doc.add_facet(schema.author, &format!("/author/{}", book.author.name));
            doc.add_text(schema.path, book.archive.path);
            doc.add_i64(schema.likes, book.likes);
            doc.add_i64(schema.dislikes, book.dislikes);
            doc.add_i64(schema.words, book.words);

            if book.likes > 0 && book.dislikes >= 0 {
                let (lower, _upper) = wilson_bounds(book.likes as f64, book.dislikes as f64);
                doc.add_f64(schema.wilson, lower);
            } else {
                doc.add_f64(schema.wilson, 0.0);
            }

            doc.add_facet(schema.status, &format!("/status/{}", book.status));
            doc.add_facet(schema.rating, &format!("/rating/{}", book.rating));

            for t in book.tags {
                doc.add_facet(schema.tag, &format!("/tag/{}", t.name));
            }

            index_writer.add_document(doc);
        }
    }

    index_writer.commit().unwrap();
    Ok(())
}

#[async_std::main]
async fn main() {
    let schema = FimfArchiveSchema::new();
    
    let index = Index::open_in_dir("index").unwrap();

    //import_fimfarchive("/home/csos95/.config/fimr/index.json", &index, &schema, 200_000).unwrap();

    let reader = index
        .reader_builder()
        .reload_policy(ReloadPolicy::OnCommit)
        .try_into().unwrap();

    println!("What is your query?");

    let stdin = std::io::stdin();
    let input = stdin.lock().lines().next().unwrap().unwrap();

    println!("Results limit?");

    let stdin = std::io::stdin();
    let limit_str = stdin.lock().lines().next().unwrap().unwrap();
    let limit: usize = limit_str.parse().expect("expected a usize");

    search(input, limit, &index, &schema, &reader);

    //let pool = SqlitePool::connect("ereader.sqlite").await.unwrap();
    //let start = chrono::Utc::now();
    //scan::scan(&pool, "epub").await.unwrap();
    //let end = chrono::Utc::now();
    //println!("start {}\nend {}\ndiff {}", start, end, end - start);
    //pool.close().await;

    //let mut siv = Cursive::new();

    //let model = init().await.unwrap();
    //view(&mut siv, &model);
    //siv.set_user_data(model);

    //siv.add_global_callback('q', |s| {
    //    cleanup(s);
    //});
    //siv.add_global_callback('l', |s| {
    //    s.cb_sink()
    //        .send(Box::new(move |s| update_view(s, Msg::GoLibrary)))
    //        .unwrap();
    //});
    //siv.run();
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
    TableOfContents(Vec<Toc>, Hyphenated),
    Bookmarks(Vec<Bookmark>, Vec<Book>),
}

enum Msg {
    GoLibrary,
    GoChapterIndex(Hyphenated, i64),
    GoChapterId(Hyphenated),
    GoChapterIdBookmark(Hyphenated, f32),
    NextChapter,
    PrevChapter,
    GoTOC,
    Scan,
    GoBookmarks,
    DeleteBookmark(i64),
    SetBookmark(Hyphenated, Hyphenated, f32),
}

async fn init() -> Result<Model, Error> {
    let pool = SqlitePool::connect("ereader.sqlite").await?;

    let books = library::get_books(&pool).await?;

    Ok(Model {
        pool,
        page: Page::Library(books),
    })
}

fn cleanup(s: &mut Cursive) {
    let model: Model = s.take_user_data().unwrap();

    task::block_on(async { model.pool.close().await });

    s.quit();
}

fn update_view(s: &mut Cursive, msg: Msg) {
    let model: Model = s.take_user_data().unwrap();

    match update(msg, model.clone()) {
        Ok(model) => {
            s.pop_layer();
            view(s, &model);

            s.set_user_data(model);
        }
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
            task::block_on(async { scan::scan(pool, "epub").await })?;
            page
        }
        (Msg::GoBookmarks, _) => {
            let (bookmarks, books) = task::block_on(async {
                let bookmarks = library::get_bookmarks(pool).await?;
                let mut books = Vec::new();
                for bookmark in &bookmarks {
                    let book = library::get_book(pool, bookmark.book_id).await?;
                    books.push(book);
                }
                Result::<(Vec<Bookmark>, Vec<Book>), Error>::Ok((bookmarks, books))
            })?;
            Page::Bookmarks(bookmarks, books)
        }
        (Msg::SetBookmark(book_id, chapter_id, progress), Page::Chapter(chapter, _)) => {
            task::block_on(async {
                library::insert_bookmark(
                    pool,
                    &library::Bookmark {
                        id: 0,
                        book_id,
                        chapter_id,
                        progress,
                        created: chrono::Utc::now(),
                    },
                )
                .await
            })?;
            Page::Chapter(chapter, Some(progress))
        }
        (Msg::DeleteBookmark(chapter_id), Page::Bookmarks(_, _)) => {
            let (bookmarks, books) = task::block_on(async {
                library::delete_bookmark(pool, chapter_id).await?;
                let bookmarks = library::get_bookmarks(pool).await?;
                let mut books = Vec::new();
                for bookmark in &bookmarks {
                    let book = library::get_book(pool, bookmark.book_id).await?;
                    books.push(book);
                }
                Result::<(Vec<Bookmark>, Vec<Book>), Error>::Ok((bookmarks, books))
            })?;
            Page::Bookmarks(bookmarks, books)
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
        Page::Bookmarks(bookmarks, books) => view_bookmarks(s, bookmarks, books),
    }
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

    view.set_on_submit(|s: &mut Cursive, id: &Hyphenated| {
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

#[allow(dead_code)]
pub fn log(message: String) {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .append(true)
        .open("debug.log")
        .unwrap();

    writeln!(file, "{}", message).unwrap()
}

fn view_chapter(s: &mut Cursive, chapter: &Chapter, progress: Option<f32>) {
    let cursor = std::io::Cursor::new(chapter.content.clone());
    let content = zstd::stream::decode_all(cursor).unwrap();
    let content_str = String::from_utf8(content).unwrap();
    let mut view = MarkupView::html(&content_str);
    view.on_link_focus(|_s, _url| {});
    view.on_link_select(|_s, _url| {});

    let mut scrollable = view.scrollable();
    if let Some(progress) = progress {
        let x = std::cmp::min(s.screen_size().x - 6, 86);
        scrollable.layout(XY::new(x, 65));

        let size = scrollable.inner_size();
        let offset_y = (size.y as f32 * progress).round() as usize;
        scrollable.set_offset(XY::new(0, offset_y));
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
        let (viewport, size) = s
            .call_on_name(
                "reader",
                |view: &mut ScrollView<MarkupView<RichRenderer>>| {
                    (view.content_viewport(), view.inner_size())
                },
            )
            .unwrap();
        let progress = viewport.top() as f32 / size.y as f32;
        s.cb_sink()
            .send(Box::new(move |s| {
                update_view(s, Msg::SetBookmark(b_id, c_id, progress))
            }))
            .unwrap();
    });

    s.add_layer(dialog.max_width(90));
}

fn view_toc(s: &mut Cursive, toc: &[Toc], book_id: Hyphenated) {
    let mut view = SelectView::new();

    for toc in toc {
        view.add_item(toc.title.clone(), toc.chapter_id);
    }

    if toc.is_empty() {
        view.add_item(
            "No table of contents. Go to start.".to_string(),
            Hyphenated::from_uuid(Uuid::nil()),
        );
    }

    view.set_on_submit(move |s, id| {
        let c_id = *id;
        s.cb_sink()
            .send(Box::new(move |s| {
                if c_id == Hyphenated::from_uuid(Uuid::nil()) {
                    update_view(s, Msg::GoChapterIndex(book_id, 1));
                } else {
                    update_view(s, Msg::GoChapterId(c_id));
                }
            }))
            .unwrap();
    });

    s.add_layer(
        Dialog::around(view.scrollable())
            .title("Table of Contents")
            .max_width(90),
    );
}

fn view_bookmarks(s: &mut Cursive, bookmarks: &[Bookmark], books: &[Book]) {
    let mut view: SelectView<(Hyphenated, f32)> = SelectView::new();

    for i in 0..bookmarks.len() {
        view.add_item(
            &books[i].title,
            (bookmarks[i].chapter_id, bookmarks[i].progress),
        );
    }

    if bookmarks.is_empty() {
        view.add_item(
            "No bookmarks. Go to library.".to_string(),
            (Hyphenated::from_uuid(Uuid::nil()), 0.0),
        );
    }

    view.set_on_submit(move |s, (id, progress)| {
        let c_id = *id;
        let c_progress = *progress;
        s.cb_sink()
            .send(Box::new(move |s| {
                if c_id == Hyphenated::from_uuid(Uuid::nil()) {
                    update_view(s, Msg::GoLibrary);
                } else {
                    update_view(s, Msg::GoChapterIdBookmark(c_id, c_progress));
                }
            }))
            .unwrap();
    });

    let mut dialog = Dialog::around(view.with_name("bookmarks").scrollable());

    dialog.add_button("Delete", move |s| {
        let bookmark = s
            .call_on_name("bookmarks", |view: &mut SelectView<(i64, f32)>| {
                view.selection().unwrap()
            })
            .unwrap();
        let id = bookmark.0;
        s.cb_sink()
            .send(Box::new(move |s| update_view(s, Msg::DeleteBookmark(id))))
            .unwrap();
    });

    s.add_layer(dialog.title("Bookmarks").max_width(90));
}
