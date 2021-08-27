
## Installation

Requirements:
- libsqlite3
- rust

1. clone repository and cd into it
2. initialize database with `sqlite3 ereader.sqlite < schema.sql`
3. compile project with `DATABASE_URL=sqlite://./ereader.sqlite cargo build --release`
4. put epub files in a directory named `epub`
5. run the project with `target/release/ereader`

## Todo
- [x] add file hash to the books table
- [x] scan for books and hash them with blake3
- [x] use the hashes to find new books
- [x] remove path from the books table and don't worry about lost/moved books?
- [x] get the chapters from books and insert those
- [x] get the table of contents from books and insert those
- [x] switch the main application over to pulling from the database
- [x] don't scan at startup, instead have scan button on library page
- [x] show something when the table of contents is empty/don't show button
- [x] ignore empty html tags (switched to cursive-markup-rs)
- [x] test compressing chapter contents
- [x] add bookmarks that point to a chapter and progress percent
- [x] the scroll position seems to be 3 off from where the bookmark was created. Figure out if it's an issue with the creation position or scroll position.  
    The width being used for setting the scroll position was two less than the actual width.  
    Fixed by changing width of 84 to `min(s.screen_size().x, 86)`.  
    Also happens when near the bottom, so need to check the screen size y against a known value.
- [x] show book title on bookmark
- [x] add delete bookmark button
- [x] make scanning faster  
    Right now the scanning reads all books, hashes, parses, and then inserts them.  
    If I switch to async io functions I might be able to use Stream (from std or futures? probably futures since it isn't nightly and has StreamExt) to read, hash, parse, and insert books as a stream.  
    This might improve scan time because it could do processing while waiting for io and it could reduce memory usage since it won't have to hold the data for all books.  
    If I switch to using uuid for the primary keys, I could generate the ids in app instead of having to wait until they're inserted and selecting last_insert_rowid.  
    I could:  
    1. get books from library
    2. put the hashes in a hashset
    3. traverse the epub directory (use walkdir)
    4. `map` the paths to (hash, buffer)
    5. `filter` out the ones that are in the hashset
    6. `map` the remaining ones to book, chapter, and toc (no need for source versions since the only thing missing was ids and the uuid would be generated in app)
    7. `for_each` to insert each  
    Doing it this way, it should be a stream and not take up as much memory.  
    [Blog post on using streams](https://gendignoux.com/blog/2021/04/01/rust-async-streams-futures-part1.html)
- [x] fix chapter uuid duplicate  
    When testing out importing all of fimfarchive, there was a duplicate chapter id.  
    I thought that filtering out duplicate files by hash, creating a book id from nil and the book file, and creating a chapter id from the book id and chapter contents would prevent collisions, but apparently not.  
    I forgot about the fact that two chapters could have the same contents.  
    Added another level of uuid with the chapter index to avoid collisions.
- [ ] try using tantivy to store fimfarchive index.json data
    - [x] load data from index.json
    - [x] make tantivy index
    - [x] put title, author, description, path in tantivy index
        text
    - [x] put likes, dislikes, wilson, words in index
        i64 for all but wilson which is f64
    - [x] put status, rating in index
        facet
    - [x] put tags in index
        facet
    - [x] search on title and description
    - [x] parse out "author(csos95)" to search on author
        facet term query  
	if there are multiple authors, make a boolean subquery and use Occur::Should on them  
        if an author name contains a closing parenthesis, escape it with one backslack
    - [x] parse out "#(Comedy)" to search on tag
        facet term query
	use "-#(Comedy)" to exclude a tag
    - [ ] parse out "words>1000" to search on words (do other comparisons too)
        range query
    - [ ] do the same for likes, dislikes, wilson
        range query
    - [ ] parse out "rating:Everyone" and "status:Complete" to search on rating and status
        facet term query
    - [ ] parse out "order:likes" to sort by likes (do same for dislikes, wilson, words)
        use TopDocs::order_by_fast_field
- [ ] benchmark the speed/storage size of different zstd levels.
- [ ] test dictionary trainging for compression  
    Training on all books would probably take too long, but try it anyways.  
    Will probably want to do something like train on the first n chapters of content.  
    Alternatively:  
    1. do the initial scan, process, and insert of books
    2. have a function to manually recompress chapters with a dictionary.
        It would:  
        1. `select id, content from chapters`
	2. decompress them all
	3. train a dictionary on it all
	4. store the dictionary in a settings table
	5. recompress them all with the dictionary
	6. update the chapters
	7. probably need to vacuum since the majority of the data in the database will have just been overwritten  
    There could also be settings for how large to make the dictionary, how many chapters to train it on, and how to select the chapters to train on (such as first n, largest n, smallest n, random n).
- [ ] clean up the mess from adding bookmarks
- [ ] make illegal states unrepresentable and clean things up 

## Features Todo
- book
    - [x] scan epub directory
    - [x] get metadata from epubs
    - [x] add to database
    - [x] compare found books to library books and only insert new ones  
        hash all but the path, put books in a hashmap of hash => book  
            then, find the difference between the two maps and  
            for found books not in library, add to library  
            for library books not found, display errors  
            for found books in library, check that the paths match, if they do not, update the path in the library  
    - [ ] full text search  
        Trying to do full text search on all of fimfarchive didn't work out because of how large it is.  
        It took up about 19GB and searching it was unusably slow.  
	The library of a user would be *much* smaller so the storage space and speed would be less likely to be an issue.  
	There are essentially two main options for what to use for this searching.  
	1. Sqlite3 FTS5  
	    Pros:  
	    - uses the existing database that stores the books  
	    Cons:  
	    - more storage space  
	    - slower  
	2. Tantivy  
	    Pros:  
	    - less storage space  
	    - faster  
	    Cons:  
	    - adds an extra file  
	    - need to keep two systems in sync when adding books to the library  
	I think that the overhead of implementing both would be similar so I may do both and see which works best.  
	I could have a trait that defines the insert/query interface and implement it for sqlite and tantivy.  
	Then I could test it out with my calibre library.  
	If both work pretty similarly, I could test it out on all of fimfarchive and see if either one handles it well.  
	I'm pretty certain sqlite will not, but maybe tantivy could.  
	[Sqlite3 FTS5](https://www.sqlite.org/fts5.html) [Tantivy query syntax](https://docs.rs/tantivy/0.15.3/tantivy/query/struct.QueryParser.html)
- library
    - [x] list books in library
    - [x] select book
- reader (should mostly be copy from first project, might change how chapter is gotten)
    - [x] get chapter from epub
    - [x] parse and style the html
    - [x] display in scroll view
    - [x] prev/next chapter
    - [x] table of contents
    - [x] ignore empty paragraph html tags (or all empty html tags?)
    - [x] show something when there is no table of contents or just don't add button?
    - [x] bookmark
- bookmark
    - [x] list
    - [x] select to go to chapter/location in book
    - [x] delete
- fimfarchive (first two should mostly be copy from old project)
    - [ ] get author, tag, story, and story_tag from index.json
    - [ ] search for fics
    - [ ] copy epub to library
- bookshelf
    - [ ] have add books menu that lists books not currently in
    - [ ] have add to bookshelf button on books
    - [ ] have remove button in bookshelf
- import to database
    - [x] store chapters in database
    - [x] store table of contents in database
- gutenberg
    - [ ] download catalog file from project gutenberg
    - [ ] parse and store metadata/epub download
    - [ ] search for books
    - [ ] copy epub to library
- subcommands
    - [ ] export bookmarks  
	If I want to be able to export bookmarks, delete the database, rescan, and import bookmarks, the ids need to be consistent.  
        Therefore, I'll need to use uuid v5.  
	I could have a "root" uuid v5 made from the nil uuid and app name.  
	Then, I could create uuid v5 for the books and chapters using the root and file bytes for books and the root and contents for chapters.  
	The other tables like bookmarks and table_of_contents don't need to have consistent ids so I can use uuid v4 for them.
    - [ ] import bookmarks
    - [ ] scan
