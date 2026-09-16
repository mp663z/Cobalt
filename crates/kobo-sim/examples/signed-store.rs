//! Create/update an explicitly local signed Store fixture; never a release key.
use kobo_app_store::{
    build_bundle, derive_public_key, sign, Catalog, CatalogEntry, CatalogEntryInput, DataKindInput,
    Manifest, ManifestInput, QualityInput,
};
use std::fs;
use std::os::unix::fs::DirBuilderExt;
use std::path::Path;

const SEED: [u8; 32] = [71; 32]; // Public, reproducible test material only.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 4 || !matches!(args[1].as_str(), "init" | "publish") {
        return Err("usage: signed-store <init|publish> DIRECTORY APP_VERSION".into());
    }
    let root = Path::new(&args[2]);
    let key = derive_public_key(&SEED)?.to_hex();
    if args[1] == "init" {
        fs::DirBuilder::new().mode(0o700).create(root)?;
        for name in ["installed", "transport"] {
            fs::DirBuilder::new().mode(0o700).create(root.join(name))?;
        }
        fs::write(root.join("format"), b"cobalt.simulator-app-store.v1\n")?;
        fs::write(root.join("key.hex"), &key)?;
    } else if fs::read(root.join("format"))? != b"cobalt.simulator-app-store.v1\n"
        || fs::read_to_string(root.join("key.hex"))? != key
    {
        return Err("refusing to modify an unrelated directory".into());
    }
    // KOBO_FIXTURE_PAYLOAD points at a real application binary (the
    // quality-fixture example) so installs can face the launch canary;
    // without it the payload stays inert bytes for pure transaction checks.
    let binary = std::env::var_os("KOBO_FIXTURE_PAYLOAD").map_or_else(
        || format!("Original inert local Store fixture {}\n", args[3]).into_bytes(),
        |path| fs::read(path).expect("fixture payload readable"),
    );
    let manifest = Manifest::new_public(ManifestInput {
        // The Store journey renders purposes, retention and update diffs from
        // this block, so the fixture carries a complete quality manifest.
        quality: Some(QualityInput {
            user: "Somebody checking the Store journey.".into(),
            job: "Stand in for a real application in install and removal checks.".into(),
            offline: "Everything the fixture does works without the network.".into(),
            data: vec![DataKindInput {
                kind: "the fixture's saved state".into(),
                location: "the app's private storage on the reader".into(),
                export: "not offered; the bytes are inert".into(),
                on_remove: "retained".into(),
            }],
            capabilities_required: vec![],
            capabilities_optional: vec![],
            profiles: vec!["clara-bw-391".into()],
            maintainer: "The Cobalt app maintainers.".into(),
            support: "the Cobalt issue tracker".into(),
            non_goals: vec!["It does nothing a real application does.".into()],
        }),
        id: "quality-fixture".into(),
        display_name: "Quality fixture".into(),
        short_label: "Fixture".into(),
        summary: "Original local package for install and recovery checks.".into(),
        version: args[3].clone(),
        minimum_cobalt_version: env!("CARGO_PKG_VERSION").into(),
        glyph: "note".into(),
        capabilities: vec![],
        binary_sha256: kobo_net::sha256::hex_digest(&binary),
        binary_bytes: binary.len() as u64,
    })?;
    let package = build_bundle(&manifest, &binary, &SEED)?;
    let package_url = format!(
        "https://fixture.invalid/{}.cobalt-app",
        kobo_net::sha256::hex_digest(&package)
    );
    let catalog = Catalog::new(vec![CatalogEntry::new(CatalogEntryInput {
        manifest,
        package_url: package_url.clone(),
        package_sha256: kobo_net::sha256::hex_digest(&package),
        package_bytes: package.len() as u64,
    })?])?
    .to_canonical_bytes();
    let write = |url: &str, body: &[u8]| {
        let path = root.join("transport").join(format!(
            "{}.blob",
            kobo_net::sha256::hex_digest(url.as_bytes())
        ));
        let staged = path.with_extension("new");
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staged)?;
        std::io::Write::write_all(&mut file, body)?;
        file.sync_all()?;
        fs::rename(staged, path)
    };
    write(&package_url, &package)?;
    write(kobod::app_store::BETA_CATALOG_URL, &catalog)?;
    write(
        kobod::app_store::BETA_CATALOG_SIGNATURE_URL,
        sign(&catalog, &SEED)?.to_hex().as_bytes(),
    )?;
    fs::File::open(root.join("transport"))?.sync_all()?;
    println!(
        "Local signed Store fixture {} at {}",
        args[3],
        root.display()
    );
    Ok(())
}
