use binrw::BinRead;
use criterion::{Criterion, criterion_group, criterion_main};
use hdk_archive::sharc::structs::SharcArchive;

#[cfg(feature = "rayon")]
use rayon::prelude::*;

fn bench_reading(c: &mut Criterion) {
    let mut group = c.benchmark_group("File Reading");
    let path = "test-data/coredata.sharc";

    group.bench_function("std::fs::read", |b| {
        b.iter(|| {
            let data = std::fs::read(path).expect("failed to read SHARC file");

            // Do some dumb stuff
            let _ = std::hint::black_box(data.iter().sum::<u8>());
        })
    });

    #[cfg(feature = "memmap2")]
    group.bench_function("memmap2", |b| {
        b.iter(|| {
            let file = std::fs::File::open(path).expect("failed to open SHARC file");
            let data =
                unsafe { memmap2::Mmap::map(&file).expect("failed to memory-map SHARC file") };

            // Do some dumb stuff
            let _ = std::hint::black_box(data.iter().sum::<u8>());
        })
    });

    group.finish();
}

fn bench_extraction(c: &mut Criterion) {
    let sharc_bytes = include_bytes!("../test-data/coredata.sharc");
    let key = [
        0x2F, 0x5C, 0xED, 0xA6, 0x3A, 0x9A, 0x67, 0x2C, 0x03, 0x4C, 0x12, 0xE1, 0xE4, 0x25, 0xFA,
        0x81, 0x16, 0x16, 0xAE, 0x1C, 0xE6, 0x6D, 0xEB, 0x95, 0xB7, 0xE6, 0xBF, 0x21, 0x40, 0x47,
        0x02, 0xDC,
    ];
    let mut reader = std::io::Cursor::new(sharc_bytes);
    let sharc = SharcArchive::read_be_args(&mut reader, (key, sharc_bytes.len() as u32)).unwrap();

    let mut group = c.benchmark_group("Extraction");

    group.bench_function("serial", |b| {
        b.iter(|| {
            let mut reader = std::io::Cursor::new(sharc_bytes);
            let _entries: Vec<_> = sharc
                .entries
                .iter()
                .map(|e| sharc.entry_data(&mut reader, e).unwrap())
                .collect();
        })
    });

    #[cfg(feature = "rayon")]
    group.bench_function("parallel", |b| {
        b.iter(|| {
            // Use a chunk size to reduce thread coordination overhead
            let _entries: Vec<_> = sharc
                .entries
                .par_iter()
                .map(|e| {
                    let mut local_reader = std::io::Cursor::new(sharc_bytes);
                    sharc.entry_data(&mut local_reader, e).unwrap()
                })
                .collect();
        })
    });

    group.finish();
}

criterion_group!(benches, bench_reading, bench_extraction);
criterion_main!(benches);
