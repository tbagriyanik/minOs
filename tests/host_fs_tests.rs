//! Dosya sistemi suruculerinin (ext4/ext2 uyumlu ve exFAT) bellek ici blok
//! aygiti uzerinde uctan uca testleri: bicimlendir -> bagla -> yaz -> oku ->
//! sil -> yeniden bagla.

use alloc::vec::Vec;

use crate::block::MemoryBlockDevice;
use crate::exfat_core;
use crate::exfat_rw::ExfatFs;
use crate::ext4_rw::{Ext4Error, Ext4Fs};

fn pattern(len: usize, seed: u8) -> Vec<u8> {
    (0..len)
        .map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed))
        .collect()
}

// ---------------------------------------------------------------------------
// exFAT
// ---------------------------------------------------------------------------

fn formatted_exfat(sectors: u64) -> MemoryBlockDevice {
    let mut dev = MemoryBlockDevice::new(sectors);
    assert!(
        exfat_core::format_device(&mut dev, 0),
        "exFAT bicimlendirme basarisiz"
    );
    dev
}

#[test]
fn exfat_format_writes_valid_vbr_and_checkup() {
    let dev = formatted_exfat(8192);
    let vbr = dev.sector_bytes(0, 512);
    assert_eq!(&vbr[3..11], b"EXFAT   ");
    assert_eq!(vbr[108], 9); // 512 bayt sektor
    assert_eq!(vbr[110], 1); // tek FAT
    assert_eq!(&vbr[510..512], &[0x55, 0xAA]);

    // Checksum sektoru (11) ile VBR dizisinin (0..10) hesabi ayni olmali.
    // Dizinin tamami (sektor 0'daki istisna alanlari haric) tek birikimli
    // toplamla hesaplanir; alanistisnalari yalnizca sektor 0 icin gecerlidir.
    let mut expect = 0u32;
    for sector in 0..11u64 {
        let bytes = dev.sector_bytes(sector, 512);
        for (i, &b) in bytes.iter().enumerate() {
            if sector == 0 && (i == 106 || i == 107 || i == 112) {
                continue;
            }
            // Rotasyon: (c >> 1) + (c << 31); `(c & 1) << 31` bit 31'i kaybeder.
            expect = (expect >> 1)
                .wrapping_add((expect & 1) << 31)
                .wrapping_add(b as u32);
        }
    }
    let stored_bytes = dev.sector_bytes(11, 4);
    let stored = u32::from_le_bytes([
        stored_bytes[0],
        stored_bytes[1],
        stored_bytes[2],
        stored_bytes[3],
    ]);
    assert_eq!(
        stored,
        exfat_core::exfat_boot_checksum_vbr_sequence(
            &vbr,
            &dev.sector_bytes(1, 512),
            &dev.sector_bytes(9, 512)
        )
    );

    // Yedek VBR (12..23) ana VBR ile ayni olmali.
    assert_eq!(dev.sector_bytes(0, 512), dev.sector_bytes(12, 512));
    assert_eq!(dev.sector_bytes(11, 512), dev.sector_bytes(23, 512));
}

#[test]
fn exfat_upcase_table_is_well_formed() {
    let table = exfat_core::build_upcase_table();
    // 0xFFFF sinir isaretleyici + 65536 UTF-16 birimi (tablo 2 bayt katidir).
    assert_eq!(table.len(), 2 + 65536 * 2);
    let first = u16::from_le_bytes([table[0], table[1]]);
    assert_eq!(first, 0xFFFF);
    // 'a' (0x0061) -> 'A' (0x0041); 'ı' -> 'I'; 'ş' -> 'Ş'
    let idx = 2 + 0x61 * 2;
    assert_eq!(u16::from_le_bytes([table[idx], table[idx + 1]]), 0x0041);
    let idx = 2 + 0x0131 * 2;
    assert_eq!(u16::from_le_bytes([table[idx], table[idx + 1]]), 0x0049);
    let idx = 2 + 0x015F * 2;
    assert_eq!(u16::from_le_bytes([table[idx], table[idx + 1]]), 0x015E);
    // Checksum deterministik olmali.
    let a = exfat_core::upcase_checksum(&table);
    let b = exfat_core::upcase_checksum(&table);
    assert_eq!(a, b);
}

#[test]
fn exfat_root_dir_has_system_entries() {
    let dev = formatted_exfat(8192);
    let fs = ExfatFs::mount(dev, 0).expect("exFAT baglanmadi");
    let root = fs.list_root();
    assert!(root.is_empty(), "kok dizinde kullanici girdisi olmamali");
    assert!(fs.bitmap_cluster >= 2);
    assert!(fs.upcase_cluster >= 2);
    assert!(fs.root_cluster > fs.upcase_cluster);
}

#[test]
fn exfat_write_read_roundtrip_across_clusters() {
    let dev = formatted_exfat(16384);
    let mut fs = ExfatFs::mount(dev, 0).unwrap();

    let small = b"Merhaba minOs!";
    assert!(fs.write_root_file("hello.txt", small));
    assert_eq!(fs.read_file("/hello.txt").as_deref(), Some(&small[..]));

    // Kume boyutundan buyuk (birkac kume) dosya
    let big = pattern(40_000, 7);
    assert!(fs.write_root_file("big.bin", &big));
    let back = fs.read_file("/big.bin").expect("buyuk dosya okunamadi");
    assert_eq!(back.len(), big.len());
    assert_eq!(back, big);

    // Ayni dosyaya yeniden yazma (kucultme): eski kumeler serbest kalmali
    let smaller = pattern(100, 3);
    assert!(fs.write_root_file("big.bin", &smaller));
    assert_eq!(fs.read_file("/big.bin").as_deref(), Some(&smaller[..]));

    // Listeleme
    let names: Vec<_> = fs.list_root().into_iter().map(|e| e.name).collect();
    assert!(names.iter().any(|n| n == "hello.txt"));
    assert!(names.iter().any(|n| n == "big.bin"));
}

#[test]
fn exfat_directories_and_unicode_names() {
    let dev = formatted_exfat(16384);
    let mut fs = ExfatFs::mount(dev, 0).unwrap();
    assert!(fs.create_dir(fs.root_cluster, "Belgeler"));
    let docs = fs.lookup("/Belgeler").expect("dizin bulunamadi");
    assert!(docs.is_dir);

    let data = b"not icerigi";
    assert!(fs.write_file(docs.first_cluster, "not defteri.txt", data));
    assert_eq!(
        fs.read_file("/Belgeler/not defteri.txt").as_deref(),
        Some(&data[..])
    );

    // Unicode ad (UTF-16) ve buyuk/kucuk harf duyarsiz arama
    assert!(fs.write_file(docs.first_cluster, "Özet.md", b"# ozet"));
    assert_eq!(
        fs.read_file("/Belgeler/Özet.md").as_deref(),
        Some(&b"# ozet"[..])
    );
    assert!(fs.lookup("/Belgeler/özet.MD").is_some());

    // Silme
    assert!(fs.remove(docs.first_cluster, "not defteri.txt"));
    assert!(fs.read_file("/Belgeler/not defteri.txt").is_none());
    assert_eq!(
        fs.read_file("/Belgeler/Özet.md").as_deref(),
        Some(&b"# ozet"[..])
    );
}

#[test]
fn exfat_content_survives_remount() {
    let mut dev = formatted_exfat(16384);
    let payload = pattern(9000, 11);
    {
        let mut fs = ExfatFs::mount(&mut dev, 0).unwrap();
        assert!(fs.write_root_file("kalici.dat", &payload));
    }
    let bytes = dev.bytes().to_vec();
    let dev2 = MemoryBlockDevice::from_bytes(&bytes);
    let fs = ExfatFs::mount(dev2, 0).unwrap();
    assert_eq!(fs.read_file("/kalici.dat"), Some(payload));
}

#[test]
fn exfat_mount_rejects_foreign_image() {
    let dev = MemoryBlockDevice::new(2048);
    assert!(ExfatFs::mount(dev, 0).is_none());
}

// ---------------------------------------------------------------------------
// ext4 (ext2 uyumlu blok esleme)
// ---------------------------------------------------------------------------

fn formatted_ext4(sectors: u64) -> MemoryBlockDevice {
    let mut dev = MemoryBlockDevice::new(sectors);
    assert!(
        crate::ext4_core::format_device(&mut dev, 0),
        "ext4 bicimlendirme basarisiz"
    );
    dev
}

#[test]
fn ext4_format_and_mount() {
    let dev = formatted_ext4(32768);
    let fs = Ext4Fs::mount(dev, 0).expect("ext4 baglanmadi");
    assert_eq!(fs.sb.block_size, 1024);
    assert_eq!(fs.sb.inodes_per_group, 2048);
    // Kök inode 2 bir dizin olmali
    let root = fs.read_inode(2).unwrap();
    assert!(fs.inode_is_dir(&root));
    assert!(fs.dir_entries(&root).unwrap().is_empty());
}

#[test]
fn ext4_write_read_roundtrip() {
    let dev = formatted_ext4(65536);
    let mut fs = Ext4Fs::mount(dev, 0).unwrap();

    let small = b"ext4 yazma testi";
    fs.write_file("/small.txt", small).unwrap();
    assert_eq!(fs.read_file("/small.txt").unwrap(), small);

    // Direct blok siniri (12 KiB) asan dosya: single indirect kullanilir.
    let big = pattern(40_000, 9);
    fs.write_file("/big.bin", &big).unwrap();
    assert_eq!(fs.read_file("/big.bin").unwrap(), big);

    // Uzerine yazma / kucultme
    let small2 = pattern(600, 1);
    fs.write_file("/big.bin", &small2).unwrap();
    assert_eq!(fs.read_file("/big.bin").unwrap(), small2);
}

#[test]
fn ext4_mkdir_nested_files_and_remove() {
    let dev = formatted_ext4(65536);
    let mut fs = Ext4Fs::mount(dev, 0).unwrap();

    let etc = fs.mkdir("/etc").unwrap();
    let nested = fs.mkdir("/etc/minos").unwrap();
    assert!(etc >= 11 && nested >= 11);

    fs.write_file("/etc/minos/api.conf", b"port=8080\n")
        .unwrap();
    assert_eq!(fs.read_file("/etc/minos/api.conf").unwrap(), b"port=8080\n");

    let names: Vec<_> = fs
        .dir_entries(&fs.read_inode(nested).unwrap())
        .unwrap()
        .into_iter()
        .map(|(_, n, _)| alloc::string::String::from(n))
        .collect();
    assert_eq!(names, alloc::vec![alloc::string::String::from("api.conf")]);

    fs.remove_file("/etc/minos/api.conf").unwrap();
    assert!(matches!(
        fs.read_file("/etc/minos/api.conf"),
        Err(Ext4Error::NotFound)
    ));

    // Dosyayi olusturup yeniden okuma
    fs.write_file("/etc/minos/api.conf", b"port=9090\n")
        .unwrap();
    assert_eq!(fs.read_file("/etc/minos/api.conf").unwrap(), b"port=9090\n");
}

#[test]
fn ext4_large_file_uses_double_indirect() {
    let dev = formatted_ext4(65536);
    let mut fs = Ext4Fs::mount(dev, 0).unwrap();
    // 12 direct + 256 single indirect = 268 blok = 274_432 bayt; ustu cift indirect.
    let big = pattern(300_000, 5);
    fs.write_file("/huge.bin", &big).unwrap();
    let back = fs.read_file("/huge.bin").unwrap();
    assert_eq!(back.len(), big.len());
    assert_eq!(back[..1000], big[..1000]);
    assert_eq!(back[290_000..], big[290_000..]);
}

#[test]
fn ext4_stats_track_allocations() {
    let dev = formatted_ext4(65536);
    let mut fs = Ext4Fs::mount(dev, 0).unwrap();
    let before = fs.stats();
    fs.write_file("/a.bin", &pattern(5000, 0)).unwrap();
    let after = fs.stats();
    assert_eq!(before[1] - after[1], 5, "5 blok ayrilmis olmali");
    assert_eq!(before[3] - after[3], 1, "1 inode ayrilmis olmali");
}

#[test]
fn ext4_mount_rejects_unsupported_features() {
    let mut dev = formatted_ext4(32768);
    // feature_incompat alanina EXTENTS bayragi ekle.
    let sb_off = 1024;
    let mut raw = [0u8; 1024];
    let bytes = dev.bytes_mut();
    raw.copy_from_slice(&bytes[sb_off..sb_off + 1024]);
    let mut features = u32::from_le_bytes([raw[96], raw[97], raw[98], raw[99]]);
    features |= crate::ext4_rw::FEATURE_INCOMPAT_EXTENTS;
    raw[96..100].copy_from_slice(&features.to_le_bytes());
    dev.bytes_mut()[sb_off..sb_off + 1024].copy_from_slice(&raw);

    match Ext4Fs::mount(dev, 0) {
        Err(Ext4Error::UnsupportedFeatures(f)) => {
            assert!(f & crate::ext4_rw::FEATURE_INCOMPAT_EXTENTS != 0);
        }
        _ => panic!("extent ozellikli birim reddedilmeliydi"),
    }
}

#[test]
fn ext4_content_survives_remount() {
    let mut dev = formatted_ext4(65536);
    let payload = pattern(20_000, 4);
    {
        let mut fs = Ext4Fs::mount(&mut dev, 0).unwrap();
        fs.write_file("/kalici.bin", &payload).unwrap();
    }
    let bytes = dev.bytes().to_vec();
    let dev2 = MemoryBlockDevice::from_bytes(&bytes);
    let fs = Ext4Fs::mount(dev2, 0).unwrap();
    assert_eq!(fs.read_file("/kalici.bin").unwrap(), payload);
}

#[test]
fn ext4_fragmentation_and_deep_directory_hierarchy() {
    let dev = formatted_ext4(65536);
    let mut fs = Ext4Fs::mount(dev, 0).unwrap();

    // 1. Derin dizin hiyerarşisi oluştur
    fs.mkdir("/a").unwrap();
    fs.mkdir("/a/b").unwrap();
    fs.mkdir("/a/b/c").unwrap();
    fs.mkdir("/a/b/c/d").unwrap();

    // 2. Parçalanma (fragmentasyon) simülasyonu: ardışık küçük dosyalar yaz ve bazılarını sil
    for i in 0..10 {
        let path = alloc::format!("/a/b/c/d/frag_{}.dat", i);
        let data = pattern(1024 * (i + 1), i as u8);
        fs.write_file(&path, &data).unwrap();
    }

    // Çift numaralı dosyaları silerek serbest blok delikleri aç
    for i in (0..10).step_by(2) {
        let path = alloc::format!("/a/b/c/d/frag_{}.dat", i);
        fs.remove_file(&path).unwrap();
    }

    // 3. Delikli blokların arasına yeni büyük dosya yaz (delikleri doldurmalı)
    let big_data = pattern(25_000, 42);
    fs.write_file("/a/b/c/d/filler.bin", &big_data).unwrap();

    // 4. Kalan tek numaralı dosyaları ve yeni büyük dosyayı doğrula
    for i in (1..10).step_by(2) {
        let path = alloc::format!("/a/b/c/d/frag_{}.dat", i);
        let read = fs.read_file(&path).unwrap();
        assert_eq!(read, pattern(1024 * (i + 1), i as u8));
    }
    assert_eq!(fs.read_file("/a/b/c/d/filler.bin").unwrap(), big_data);
}

#[test]
fn exfat_fragmentation_and_cluster_chain_integrity() {
    let dev = formatted_exfat(32768);
    let mut fs = ExfatFs::mount(dev, 0).unwrap();

    // Derin dizin yapısı
    assert!(fs.create_dir(fs.root_cluster, "docs"));
    let docs = fs.lookup("/docs").unwrap();
    assert!(fs.create_dir(docs.first_cluster, "sub"));
    let sub = fs.lookup("/docs/sub").unwrap();

    // 8 adet dosya yaz (her biri küme sınırlarını aşan boyutlarda)
    for i in 0..8 {
        let name = alloc::format!("file_{}.bin", i);
        assert!(fs.write_file(sub.first_cluster, &name, &pattern(6000, i as u8)));
    }

    // Bazılarını silerek FAT tablosunda parçalanma oluştur
    assert!(fs.remove(sub.first_cluster, "file_1.bin"));
    assert!(fs.remove(sub.first_cluster, "file_3.bin"));
    assert!(fs.remove(sub.first_cluster, "file_5.bin"));

    // Parçalanmış FAT üzerine büyük dosya yaz
    let big = pattern(24_000, 99);
    assert!(fs.write_file(sub.first_cluster, "merged.dat", &big));

    // Bütünlüğü doğrula
    assert_eq!(fs.read_file("/docs/sub/merged.dat").unwrap(), big);
    assert_eq!(
        fs.read_file("/docs/sub/file_0.bin").unwrap(),
        pattern(6000, 0)
    );
    assert_eq!(
        fs.read_file("/docs/sub/file_2.bin").unwrap(),
        pattern(6000, 2)
    );
    assert_eq!(
        fs.read_file("/docs/sub/file_4.bin").unwrap(),
        pattern(6000, 4)
    );
}
