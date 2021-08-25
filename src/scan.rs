use crate::library::{self, Book, Chapter, Toc};
use crate::Error;
use futures::stream;
use futures::StreamExt;
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

async fn get_file<P: AsRef<async_std::path::Path>>(path: P) -> Vec<u8> {
    async_std::fs::read(path).await.unwrap()
}

fn hash(buff: Vec<u8>) -> (String, Vec<u8>) {
    let hash = blake3::hash(buff.as_slice()).to_string();
    (hash, buff)
}

fn process_epub(hash: String, buff: Vec<u8>) -> (Book, Vec<Chapter>, Vec<Toc>) {
    use uuid::Uuid;

    let book_id = Uuid::new_v5(&Uuid::nil(), &buff);

    let mut doc = epub::doc::EpubDoc::from_reader(std::io::Cursor::new(buff)).unwrap();

    let spine = doc.spine.clone();
    let chapters = spine
        .into_iter()
        .enumerate()
        .map(|(i, id)| {
            let content = doc.get_resource_str(&id[..]).unwrap();
            let chapter_id = Uuid::new_v5(&book_id, content.as_bytes());
            Chapter {
                id: chapter_id,
                book_id,
                index: i as i64 + 1,
                content: zstd::stream::encode_all(content.as_bytes(), 8).unwrap(),
            }
        })
        .collect::<Vec<Chapter>>();

    let toc = doc
        .toc
        .iter()
        .enumerate()
        .map(|(index, nav)| {
            // Some TOC links have a fragment to jump to a specific spot in the chapter.
            // I need to remove that so the link can be turned into a spine index.
            let mut url =
                url::Url::parse(&format!("epub:///{}", nav.content.to_string_lossy())[..]).unwrap();
            url.set_fragment(None);

            let absolute_path = url.to_string();
            let relative_path = absolute_path.trim_start_matches("epub:///");
            let decoded_path = percent_decode_str(relative_path)
                .decode_utf8_lossy()
                .to_string();

            let mut content_path = PathBuf::new();
            content_path.push(decoded_path);

            let spine_index = doc.resource_uri_to_chapter(&content_path).unwrap() as i64;

            Toc {
                id: 0,
                book_id,
                index: index as i64,
                chapter_id: chapters[spine_index as usize].id,
                title: nav.label.clone(),
            }
        })
        .collect::<Vec<Toc>>();

    (
        Book {
            id: book_id,
            identifier: get_metadata(&doc, "identifier"),
            language: get_metadata(&doc, "language"),
            title: get_metadata(&doc, "title"),
            creator: doc.mdata("creator"),
            description: doc.mdata("description"),
            publisher: doc.mdata("publisher"),
            hash,
        },
        chapters,
        toc,
    )
}

type Epub = epub::doc::EpubDoc<std::io::Cursor<Vec<u8>>>;

fn get_metadata(doc: &Epub, tag: &str) -> String {
    doc.mdata(tag).unwrap()
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
    let library_hashes = library_hashes(pool).await.unwrap();
    let mut new_hashes = HashSet::<String>::new();

    stream::iter(entries(path))
        .map(|e| async move { get_file(e.path()).await })
        // buffering a few so there isn't a delay in reads
        .buffer_unordered(4)
        .map(hash)
        .filter_map(|(hash, buff)| {
            let result = if !library_hashes.contains(&hash) && !new_hashes.contains(&hash) {
                new_hashes.insert(hash.clone());
                Some((hash, buff))
            } else {
                None
            };
            async move { result }
        })
        .map(|(hash, buff)| process_epub(hash, buff))
        .chunks(8)
        .for_each(|books| async move {
            let mut tx = pool.begin().await.unwrap();
            for (book, chapters, toc) in books {
                library::insert_book(&mut tx, &book).await.unwrap();
                for chapter in chapters {
                    library::insert_chapter(&mut tx, &chapter).await.unwrap();
                }
                for toc in toc {
                    library::insert_toc(&mut tx, &toc).await.unwrap();
                }
            }
            tx.commit().await.unwrap();
        })
        .await;

    Ok(())
}
