extern crate seq_io;
#[macro_use]
extern crate matches;

use seq_io::fasta::*;
use std::io;

const FASTA: &[&[u8]; 11] = &[
    b">id desc",
    b"ACCGTAGGCT",
    b"CCGTAGGCTG",
    b"CGTAGGCTGA",
    b"GTAGGCTGAA",
    b"CCCC",
    b">id2",
    b"ATTGTTGTTT",
    b"ATTGTTGTTT",
    b"ATTGTTGTTT",
    b"GGGG",
];

fn concat_lines(lines: &[&[u8]], terminator: &[u8], last: bool) -> Vec<u8> {
    let mut out: Vec<_> = lines
        .iter()
        .flat_map(|s| s.iter().chain(terminator))
        .cloned()
        .collect();
    if !last {
        let l = out.len();
        out.truncate(l - terminator.len());
    }
    out
}

#[test]
fn test_fasta_reader() {
    let expected = [
        (Ok("id"), Some(Ok("desc")), (1, 6)),
        (Ok("id2"), None, (7, 11)),
    ];
    let lterms: [&[u8]; 2] = [b"\n", b"\r\n"];

    // try different line endings
    for t in &lterms {
        let fasta = concat_lines(FASTA, *t, false);
        let exp_seqs: Vec<_> = expected
            .iter()
            .map(|&(_, _, (start, end))| {
                (
                    // raw sequence
                    concat_lines(&FASTA[start..end], *t, false),
                    // concatenated sequence
                    FASTA[start..end].concat().to_vec(),
                )
            }).collect();

        // try different initial capacities to test
        // buffer growing feature
        for cap in 3..100 {
            let mut exp_iter = expected.iter().zip(&exp_seqs);
            let mut reader = Reader::with_capacity(fasta.as_slice(), cap);
            while let Some((&(id, desc, _), &(ref raw_seq, ref seq))) = exp_iter.next() {
                let record = reader
                    .next()
                    .unwrap()
                    .unwrap_or_else(|_| panic!("Error reading record at cap. {}", cap));

                assert_eq!(record.id(), id);
                assert_eq!(record.desc(), desc);
                assert_eq!(record.seq(), raw_seq.as_slice());
                assert_eq!(record.owned_seq().as_slice(), seq.as_slice());

                let owned = record.to_owned_record();
                assert_eq!(owned.id(), id);
                assert_eq!(owned.desc(), desc);
                assert_eq!(owned.seq(), seq.as_slice());
            }
            assert!(reader.next().is_none());
        }
    }
}

#[test]
fn test_fasta_seq_lines() {
    let mut reader = Reader::new(&b">id\nAT\nGC\n"[..]);
    let rec = reader.next().unwrap().unwrap();
    let lines: Vec<_> = rec.seq_lines().collect();
    assert_eq!(rec.num_seq_lines(), 2);
    assert_eq!(lines, vec![b"AT", b"GC"]);
}

#[test]
fn test_fasta_full_seq() {
    use std::borrow::Cow;
    let mut reader = Reader::new(&b">id\nATGC\n"[..]);
    let rec = reader.next().unwrap().unwrap();
    assert_matches!(rec.full_seq(), Cow::Borrowed(b"ATGC"));

    let mut reader = Reader::new(&b">id\nAT\nGC\n"[..]);
    let rec = reader.next().unwrap().unwrap();
    assert_eq!(rec.full_seq().into_owned(), b"ATGC".to_owned());
}

#[test]
fn test_fasta_invalid_start() {
    let mut reader = Reader::new(&b"\r\nid\nATGC\n"[..]);
    let rec = reader.next().unwrap();
    assert_matches!(
        rec,
        Err(Error::InvalidStart {
            line: 2,
            found: b'i'
        })
    );
}

#[test]
fn test_fasta_truncated() {
    let mut reader = Reader::new(&b">id1\nATGC\n>id2"[..]);
    reader.next().unwrap().unwrap();
    let rec = reader.next().unwrap();
    assert_matches!(rec, Err(Error::UnexpectedEnd { line: 3 }));
}

#[test]
fn test_fasta_none_after_err() {
    let mut reader = Reader::new(&b"id\nATGC\n"[..]);
    assert!(reader.next().unwrap().is_err());
    assert!(reader.next().is_none());
}

#[test]
fn test_fasta_empty() {
    let mut reader = Reader::new(&b""[..]);
    assert!(reader.next().is_none());
    let mut reader = Reader::new(&b"\n\n\n\n\n\n"[..]);
    assert!(reader.next().is_none());
}

#[test]
fn test_fasta_empty_lines_start() {
    let mut reader = Reader::new(&b"\n\n\n\n\n\n\n\n\n>id\nATGC\n"[..]);
    assert_eq!(reader.next().unwrap().unwrap().id_bytes(), b"id");
}

#[test]
fn test_fasta_empty_lines_end() {
    let mut reader = Reader::new(&b">id\nATGC\n\n\n\n\n\n\n\n\n"[..]);
    assert_eq!(reader.next().unwrap().unwrap().id_bytes(), b"id");
    assert!(reader.next().is_none());
}

#[test]
fn test_fasta_no_newline_end() {
    // up to 3 newlines at end possible
    let mut reader = Reader::new(&b">id\nATGC"[..]);
    assert_eq!(reader.next().unwrap().unwrap().id_bytes(), b"id");
    assert!(reader.next().is_none());
}

#[test]
fn test_fasta_recset() {
    let fa = &b">s1\nAT\nGC\n\n>s2\n\nATGC\n>s3\nATGC\n"[..];

    for cap in 3..400 {
        let mut reader = Reader::with_capacity(fa, cap);
        let mut rsets = vec![];
        loop {
            rsets.push(RecordSet::default());
            let rset = rsets.last_mut().unwrap();
            if let Some(r) = reader.read_record_set(rset) {
                r.unwrap();
            } else {
                break;
            }
        }
        let mut rset_iter = rsets.iter().flat_map(|r| r.into_iter());

        let mut reader = Reader::new(fa);
        while let Some(r0) = reader.next() {
            let rec = rset_iter.next().unwrap();
            let r0 = r0.unwrap();
            assert_eq!(rec.id(), r0.id());
            assert_eq!(rec.desc(), r0.desc());
            assert_eq!(rec.head(), r0.head());
            assert_eq!(rec.seq(), r0.seq());
        }
    }
}

#[test]
fn test_fasta_parallel() {
    let fa = &b">s1\nAT\nGC\n\n>s2\n\nATGC\n>s3\nATGC\n>s1\nAT\nGC\n\n>s2\n\nATGC\n>s3\nATGC\n>s1\nAT\nGC\n\n>s2\n\nATGC\n>s3\nATGC\n"[..];

    for cap in 3..400 {
        let par_reader = Reader::with_capacity(fa, cap);
        let mut reader = Reader::new(fa);

        seq_io::parallel::parallel_fasta(
            par_reader,
            1,
            2,
            |_, out| {
                *out = ();
            },
            |rec, _| {
                // runs in main thread
                let r0 = reader.next().unwrap().unwrap();
                assert_eq!(rec.id(), r0.id());
                assert_eq!(rec.desc(), r0.desc());
                assert_eq!(rec.head(), r0.head());
                assert_eq!(rec.seq(), r0.seq());
                None::<()>
            },
        ).unwrap();
    }
}

#[test]
fn test_fasta_seek() {
    let fa = b"\n\n\n\n>s1\nAT\nGC\n\n>s2\n\nATGC\n>s3\nATGC\n";
    for cap in 5..41 {
        let mut reader = Reader::with_capacity(io::Cursor::new(&fa[..]), cap);
        let rec1 = reader.next().unwrap().unwrap().to_owned_record();
        let pos1 = reader.position().unwrap().clone();
        let rec2 = reader.next().unwrap().unwrap().to_owned_record();
        let rec3 = reader.next().unwrap().unwrap().to_owned_record();
        let pos3 = reader.position().unwrap().clone();

        assert_eq!(pos1.line(), 5);
        assert_eq!(pos3.line(), 12);
        assert_eq!(pos1.byte(), 4);
        assert_eq!(pos3.byte(), 25);

        reader.seek(&pos1).unwrap();
        assert_eq!(reader.next().unwrap().unwrap().id_bytes(), rec1.id_bytes());
        assert_eq!(reader.next().unwrap().unwrap().id_bytes(), rec2.id_bytes());
        assert_eq!(reader.next().unwrap().unwrap().id_bytes(), rec3.id_bytes());

        reader.seek(&pos3).unwrap();
        assert_eq!(reader.next().unwrap().unwrap().id_bytes(), rec3.id_bytes());
        assert!(reader.next().is_none());

        reader.seek(&pos3).unwrap();
        assert_eq!(reader.next().unwrap().unwrap().id_bytes(), rec3.id_bytes());
        assert!(reader.next().is_none());
    }
}

// FASTA writing

#[test]
fn test_write_fasta() {
    let fasta = concat_lines(FASTA, b"\n", true);
    let mut reader = Reader::new(&fasta[..]);
    let mut out = vec![];
    while let Some(r) = reader.next() {
        let r = r.unwrap();
        r.write_wrap(&mut out, 10).unwrap();
    }
    assert_eq!(&out, &fasta);
}

#[test]
fn test_fasta_write_head() {
    let mut out = vec![];
    write_head(&mut out, b"id desc").unwrap();
    assert_eq!(&out, b">id desc\n");
}

#[test]
fn test_fasta_write_seq() {
    let mut out = vec![];
    write_seq(&mut out, b"ATGC").unwrap();
    assert_eq!(&out, b"ATGC\n");
}

#[test]
fn test_fasta_write_seq_wrap() {
    let mut out = vec![];
    write_wrap_seq(&mut out, b"ATGCA", 2).unwrap();
    assert_eq!(&out, b"AT\nGC\nA\n");
}

#[test]
fn test_fasta_write_seq_iter() {
    let mut out = vec![];
    write_seq_iter(&mut out, b"ATGCA".chunks(2)).unwrap();
    assert_eq!(&out, b"ATGCA\n");
}

#[test]
fn test_fasta_write_seq_iter_wrap() {
    for size in 1..11 {
        let mut out = vec![];
        write_wrap_seq_iter(&mut out, b"AAAATTTTGGG".chunks(size), 3).unwrap();
        assert_eq!(&out, b"AAA\nATT\nTTG\nGG\n");

        let mut out = vec![];
        write_wrap_seq_iter(&mut out, b"AAAATTTTGGG".chunks(size), 4).unwrap();
        assert_eq!(&out, b"AAAA\nTTTT\nGGG\n");
    }
}
