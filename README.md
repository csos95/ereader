
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
- [ ] make illegal states unrepresentable and clean things up 
- [x] ignore empty html tags (switched to cursive-markup-rs)

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
    - [ ] bookmark
- bookmark
    - [ ] list
    - [ ] select to go to chapter/location in book
    - [ ] delete
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
