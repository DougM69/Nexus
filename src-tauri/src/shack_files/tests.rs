//! The three folders' fetch, by value: what crosses is the file's own bytes, a path that leads
//! anywhere but a file the folder shares is refused, each read is bounded, and the pieces saved
//! here put the file back together.

use super::*;

fn scratch(label: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("nexus-shack-files-{label}-{nanos}"));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

fn bytes(n: usize) -> Vec<u8> {
    (0..n).map(|i| (i * 7 + i / 251) as u8).collect()
}

/// Every piece of `name` from offset 0, read as the other computer reads them.
fn read_whole(folder: Folder, dir: &Path, name: &str) -> (Vec<u8>, Vec<usize>) {
    let (mut whole, mut sizes, mut offset) = (Vec::new(), Vec::new(), 0u64);
    loop {
        let piece = read_in(folder, dir, name, offset).expect("a piece");
        assert_eq!(piece.offset, offset);
        let data = crate::b64_decode(&piece.data).expect("base64");
        sizes.push(data.len());
        offset += data.len() as u64;
        whole.extend(data);
        if offset >= piece.size {
            return (whole, sizes);
        }
    }
}

/// ★ A gallery picture arrives as its own bytes, by the path the gallery names it by.
#[test]
fn a_gallery_picture_arrives_by_its_path() {
    let dir = scratch("gallery");
    let picture = bytes(300_000);
    let path = dir.join("20261017_120000_Scottie1.png");
    std::fs::write(&path, &picture).unwrap();
    let got = gallery_image_in(&dir, path.to_str().unwrap()).expect("the picture");
    assert_eq!(crate::b64_decode(&got).unwrap(), picture);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// ★ A path that does not end at a file the folder shares is refused, by every road out: a `..`,
/// an absolute path elsewhere, a link inside the folder to a file outside it, a name with a folder
/// in it, and a file of a kind the folder does not share. CONTROL: the same file inside the
/// folder, by its own name, is read.
#[test]
fn a_path_outside_the_folder_is_refused() {
    let root = scratch("confine");
    let dir = root.join("Nexus SSTV");
    let outside = root.join("elsewhere");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::create_dir_all(outside.join("sub")).unwrap();
    std::fs::write(outside.join("secret.png"), b"not shared").unwrap();
    std::fs::write(root.join("up.png"), b"not shared").unwrap();
    std::fs::write(dir.join("gallery.json"), b"[]").unwrap();
    std::fs::write(dir.join("ok.png"), b"shared").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(outside.join("secret.png"), dir.join("link.png")).unwrap();

    let refused = |p: &str| {
        assert!(gallery_image_in(&dir, p).is_err(), "gallery read {p}");
        assert!(
            read_in(Folder::Sstv, &dir, p, 0).is_err(),
            "folder read {p}"
        );
    };
    refused("../up.png");
    refused(dir.join("../up.png").to_str().unwrap());
    refused(outside.join("secret.png").to_str().unwrap());
    refused("gallery.json");
    #[cfg(unix)]
    refused("link.png");
    // A path into a folder inside, even one that ends back in the gallery, is not a bare name.
    assert!(read_in(Folder::Sstv, &dir, "../Nexus SSTV/ok.png", 0).is_err());

    // CONTROL: the file that is shared, both ways.
    assert_eq!(
        crate::b64_decode(&gallery_image_in(&dir, dir.join("ok.png").to_str().unwrap()).unwrap())
            .unwrap(),
        b"shared"
    );
    assert_eq!(
        crate::b64_decode(&read_in(Folder::Sstv, &dir, "ok.png", 0).unwrap().data).unwrap(),
        b"shared"
    );
    let listed: Vec<String> = list_in(Folder::Sstv, &dir)
        .into_iter()
        .map(|f| f.name)
        .collect();
    assert_eq!(listed, vec!["ok.png".to_string()]);
    std::fs::remove_dir_all(&root).unwrap();
}

/// ★ ALL.TXT's folder shares ALL.TXT and nothing else: the diagnostic log and crash report beside
/// it are neither listed nor read. The recordings share their recordings only.
#[test]
fn each_folder_shares_its_own_kind_only() {
    let dir = scratch("kinds");
    std::fs::write(dir.join("ALL.TXT"), b"251017_120000 decode\n").unwrap();
    std::fs::write(dir.join("nexus-diag.log"), b"diagnostic").unwrap();
    std::fs::write(dir.join("nexus-crash.txt"), b"crash").unwrap();
    std::fs::write(dir.join("qso-KD9TAW-1.wav"), b"RIFF").unwrap();
    let names =
        |folder| -> Vec<String> { list_in(folder, &dir).into_iter().map(|f| f.name).collect() };
    assert_eq!(names(Folder::AllTxt), vec!["ALL.TXT".to_string()]);
    assert_eq!(
        names(Folder::Recordings),
        vec!["qso-KD9TAW-1.wav".to_string()]
    );
    assert!(read_in(Folder::AllTxt, &dir, "nexus-diag.log", 0).is_err());
    assert!(read_in(Folder::AllTxt, &dir, "nexus-crash.txt", 0).is_err());
    assert!(read_in(Folder::Recordings, &dir, "ALL.TXT", 0).is_err());
    assert_eq!(
        crate::b64_decode(&read_in(Folder::AllTxt, &dir, "ALL.TXT", 0).unwrap().data).unwrap(),
        b"251017_120000 decode\n"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// ★ Each read is bounded: a file two pieces and a little long comes in three, none longer than a
/// piece, and they make the file. A picture past the bound is refused whole; one at it is read.
#[test]
fn the_size_bound_holds() {
    let dir = scratch("bound");
    let long = bytes(2 * PIECE_BYTES + 5);
    std::fs::write(dir.join("ALL.TXT"), &long).unwrap();
    let (whole, sizes) = read_whole(Folder::AllTxt, &dir, "ALL.TXT");
    assert_eq!(sizes, vec![PIECE_BYTES, PIECE_BYTES, 5]);
    assert!(whole == long, "the pieces make the file");

    let big = dir.join("big.png");
    std::fs::File::create(&big)
        .unwrap()
        .set_len(IMAGE_BYTES + 1)
        .unwrap();
    assert_eq!(
        gallery_image_in(&dir, big.to_str().unwrap()),
        Err("That picture is too large to fetch".to_string())
    );
    std::fs::File::create(&big)
        .unwrap()
        .set_len(IMAGE_BYTES)
        .unwrap();
    let at = gallery_image_in(&dir, big.to_str().unwrap()).expect("a picture at the bound");
    assert_eq!(crate::b64_decode(&at).unwrap().len() as u64, IMAGE_BYTES);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// ★ The save lands here: the pieces written into this computer's Downloads folder make the
/// station's file, a save begun again replaces an older file of that name, a piece out of its
/// place is refused, and the name cannot lead out of the folder.
#[test]
fn the_save_lands_on_this_computer() {
    // Downloads sits inside a folder of this run's own, so the escape checked below would land
    // in that folder and nowhere another run (or a mutant of this test) can have written.
    let root = scratch("save");
    let shack = root.join("shack");
    let downloads = root.join("Downloads");
    std::fs::create_dir_all(&shack).unwrap();
    std::fs::create_dir_all(&downloads).unwrap();
    let file = bytes(PIECE_BYTES + 77);
    std::fs::write(shack.join("qso-KD9TAW-1.wav"), &file).unwrap();
    std::fs::write(
        downloads.join("qso-KD9TAW-1.wav"),
        b"an older, longer file of that name",
    )
    .unwrap();

    let mut offset = 0u64;
    loop {
        let piece = read_in(Folder::Recordings, &shack, "qso-KD9TAW-1.wav", offset).unwrap();
        let saved = save_piece_in(&downloads, "qso-KD9TAW-1.wav", &piece.data, offset).unwrap();
        assert_eq!(
            saved,
            downloads.join("qso-KD9TAW-1.wav").display().to_string()
        );
        offset += crate::b64_decode(&piece.data).unwrap().len() as u64;
        if offset >= piece.size {
            break;
        }
    }
    assert!(std::fs::read(downloads.join("qso-KD9TAW-1.wav")).unwrap() == file);

    // The first piece again, as a retry would send it at the wrong place.
    let first = read_in(Folder::Recordings, &shack, "qso-KD9TAW-1.wav", 0).unwrap();
    assert!(save_piece_in(&downloads, "qso-KD9TAW-1.wav", &first.data, 5).is_err());
    assert!(std::fs::read(downloads.join("qso-KD9TAW-1.wav")).unwrap() == file);

    let saved = save_piece_in(&downloads, "../escape.wav", &first.data, 0);
    assert!(saved.is_err(), "{saved:?}");
    assert!(!root.join("escape.wav").exists());
    std::fs::remove_dir_all(&root).unwrap();
}
