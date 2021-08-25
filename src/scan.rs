use crate::library::{self, Book, Chapter, Toc};
use crate::Error;
use futures::{stream, StreamExt, TryStreamExt};
use percent_encoding::percent_decode_str;
use sqlx::SqlitePool;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use walkdir::WalkDir;

fn entries<P: AsRef<Path>>(path: P) -> impl Iterator<Item = walkdir::DirEntry> {
    WalkDir::new(&path)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().unwrap_or_default() == "epub")
}

async fn get_file<P: AsRef<async_std::path::Path>>(path: P) -> Result<Vec<u8>, Error> {
    Ok(async_std::fs::read(path).await?)
}

fn hash(buff: Vec<u8>) -> (String, Vec<u8>) {
    let hash = blake3::hash(buff.as_slice()).to_string();
    (hash, buff)
}

fn process_epub(hash: String, buff: Vec<u8>) -> Result<(Book, Vec<Chapter>, Vec<Toc>), Error> {
    use uuid::Uuid;

    let book_id = Uuid::new_v5(&Uuid::nil(), &buff);

    let mut doc = epub::doc::EpubDoc::from_reader(std::io::Cursor::new(buff))?;

    let spine = doc.spine.clone();
    let chapters = spine
        .into_iter()
        .enumerate()
        .map(|(i, id)| {
            let content = doc.get_resource_str(&id[..])?;
            let chapter_id = Uuid::new_v5(&book_id, content.as_bytes());
            Ok(Chapter {
                id: chapter_id,
                book_id,
                index: i as i64 + 1,
                content: zstd::stream::encode_all(content.as_bytes(), 8)?,
            })
        })
        .collect::<Result<Vec<Chapter>, Error>>()?;

    let toc = doc
        .toc
        .iter()
        .enumerate()
        .map(|(index, nav)| {
            // Some TOC links have a fragment to jump to a specific spot in the chapter.
            // I need to remove that so the link can be turned into a spine index.
            let mut url =
                url::Url::parse(&format!("epub:///{}", nav.content.to_string_lossy())[..])?;
            url.set_fragment(None);

            let absolute_path = url.to_string();
            let relative_path = absolute_path.trim_start_matches("epub:///");
            let decoded_path = percent_decode_str(relative_path)
                .decode_utf8_lossy()
                .to_string();

            let mut content_path = PathBuf::new();
            content_path.push(decoded_path);

            let spine_index = match doc.resource_uri_to_chapter(&content_path) {
                Some(i) => Ok(i),
                None => Err(Error::EpubMissingTocResource),
            }? as i64;

            Ok(Toc {
                id: 0,
                book_id,
                index: index as i64,
                chapter_id: chapters[spine_index as usize].id,
                title: nav.label.clone(),
            })
        })
        .collect::<Result<Vec<Toc>, Error>>()?;

    Ok((
        Book {
            id: book_id,
            identifier: get_metadata(&doc, "identifier")?,
            language: get_metadata(&doc, "language")?,
            title: get_metadata(&doc, "title")?,
            creator: doc.mdata("creator"),
            description: doc.mdata("description"),
            publisher: doc.mdata("publisher"),
            hash,
        },
        chapters,
        toc,
    ))
}

type Epub = epub::doc::EpubDoc<std::io::Cursor<Vec<u8>>>;

fn get_metadata(doc: &Epub, tag: &str) -> Result<String, Error> {
    doc.mdata(tag)
        .ok_or_else(|| Error::MissingMetadata(tag.to_string()))
}

async fn library_hashes(pool: &SqlitePool) -> Result<HashSet<String>, Error> {
    let library_books = library::get_books(pool).await?;

    Ok(library_books
        .into_iter()
        .fold(HashSet::new(), |mut set, book| {
            set.insert(book.hash);
            set
        }))
}

pub async fn scan<P: AsRef<Path>>(pool: &SqlitePool, path: P) -> Result<(), Error> {
    let library_hashes = library_hashes(pool).await?;
    let mut new_hashes = HashSet::<String>::new();

    stream::iter(entries(path))
        .map(|e| async move { get_file(e.path()).await })
        // buffering a few so there isn't a delay in reads
        .buffer_unordered(4)
        .and_then(|buff| async move { Ok(hash(buff)) })
        .try_filter_map(|(hash, buff)| {
            let result = if !library_hashes.contains(&hash) && !new_hashes.contains(&hash) {
                new_hashes.insert(hash.clone());
                Some((hash, buff))
            } else {
                None
            };
            async move { Ok(result) }
        })
        .map_ok(|(hash, buff)| process_epub(hash, buff))
        .try_for_each(|result| async move {
            let (book, chapters, toc) = result?;
            let mut tx = pool.begin().await?;
            library::insert_book(&mut tx, &book).await?;
            for chapter in chapters {
                library::insert_chapter(&mut tx, &chapter).await?;
            }
            for toc in toc {
                library::insert_toc(&mut tx, &toc).await?;
            }
            tx.commit().await?;
            Ok(())
        })
        .await?;

    Ok(())
}
