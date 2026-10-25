//! Tests for signature reading and verification on synthetic packages.

use sofd_crypto::Sm2KeyPair;
use sofd_error::{OfdError, SignatureError};
use sofd_package::Package;
use sofd_sign::{
    Reference, Signature, SignatureKind, TimestampStatus, VerifyStatus, read_coverage, read_seals,
    read_sign_list, read_signature, signed_digest, verify_descriptor, verify_signature,
    verify_timestamp, verify_with_public_key,
};
use sofd_testkit::{
    DOCUMENT_ENTRY, OFD_XML_SIGNED, SIGN_FILE, SIGNS_ENTRY, SIGNS_XML, TimestampSpec, base64_of,
    descriptor, open_package, open_package_bytes, signed_descriptor_entries, signed_fixture,
    sm2_cert_der_over, timestamp_token,
};

const SIG_FILE: &str = "Signs/Sign_0.xml";

fn signs_xml() -> Vec<u8> {
    br#"<Signs><Sign BaseLoc="Signs/Sign_0.xml"/></Signs>"#.to_vec()
}

fn open(entries: &[(&str, Vec<u8>)]) -> Package {
    open_package(entries)
}

fn make_signature(entries: &[(&str, Vec<u8>)], _refs: &[(&str, &str)]) -> (Signature, Sm2KeyPair) {
    let mut package = open(entries);
    let sig = read_signature(&mut package, SIG_FILE).expect("descriptor");
    let pair = Sm2KeyPair::generate().expect("keygen");
    let mut signed = sig.clone();
    if let Ok(Some(digest)) = signed_digest(&mut package, &sig) {
        signed.check_value = pair.sign(&digest);
    }
    (signed, pair)
}

#[test]
fn sign_list_is_parsed() {
    let mut package = open(&[("Signs/Signs.xml", signs_xml())]);
    let signs = read_sign_list(&mut package, "Signs/Signs.xml").expect("list");
    assert_eq!(signs.len(), 1);
    assert_eq!(signs[0].base_loc, SIG_FILE);
}

#[test]
fn descriptor_fields_are_parsed() {
    let entries = vec![
        ("Signs/Signs.xml", signs_xml()),
        (
            SIG_FILE,
            descriptor("sig1", &[("SM3", "Doc_0/Document.xml")]).into_bytes(),
        ),
    ];
    let mut package = open(&entries);
    let sig = read_signature(&mut package, SIG_FILE).expect("descriptor");
    assert_eq!(sig.id.as_deref(), Some("sig1"));
    assert_eq!(sig.references.len(), 1);
    assert_eq!(
        sig.references[0],
        Reference {
            check_method: "SM3".into(),
            file_ref: "Doc_0/Document.xml".into(),
            check_value: Vec::new(),
        }
    );
}

#[test]
fn valid_signature_verifies() {
    let content: Vec<u8> = b"document bytes".to_vec();
    let entries = vec![
        ("Signs/Signs.xml", signs_xml()),
        (
            SIG_FILE,
            descriptor("s", &[("SM3", "Doc_0/Document.xml")]).into_bytes(),
        ),
        ("Doc_0/Document.xml", content),
    ];
    let (sig, pair) = make_signature(&entries, &[("SM3", "Doc_0/Document.xml")]);
    let mut package = open(&entries);
    assert_eq!(
        verify_with_public_key(&mut package, &sig, &pair.public_bytes()).expect("verify"),
        VerifyStatus::Valid
    );
}

#[test]
fn tampered_content_is_detected() {
    let entries = vec![
        ("Signs/Signs.xml", signs_xml()),
        (
            SIG_FILE,
            descriptor("s", &[("SM3", "Doc_0/Document.xml")]).into_bytes(),
        ),
        ("Doc_0/Document.xml", b"document bytes".to_vec()),
    ];
    let (sig, pair) = make_signature(&entries, &[("SM3", "Doc_0/Document.xml")]);
    let tampered = vec![
        ("Signs/Signs.xml", signs_xml()),
        (
            SIG_FILE,
            descriptor("s", &[("SM3", "Doc_0/Document.xml")]).into_bytes(),
        ),
        ("Doc_0/Document.xml", b"document BYTES".to_vec()),
    ];
    let mut package = open(&tampered);
    assert_eq!(
        verify_with_public_key(&mut package, &sig, &pair.public_bytes()).expect("verify"),
        VerifyStatus::Tampered
    );
}

#[test]
fn missing_reference_is_tampered() {
    let entries = vec![
        ("Signs/Signs.xml", signs_xml()),
        (
            SIG_FILE,
            descriptor("s", &[("SM3", "Doc_0/Document.xml")]).into_bytes(),
        ),
    ];
    let (sig, pair) = make_signature(&entries, &[("SM3", "Doc_0/Document.xml")]);
    let mut package = open(&entries);
    let out = verify_with_public_key(&mut package, &sig, &pair.public_bytes());
    assert!(
        matches!(out, Err(OfdError::Signature(_))),
        "missing file fails"
    );
}

#[test]
fn verify_without_certificate_reports_unchecked() {
    let entries = vec![
        ("Signs/Signs.xml", signs_xml()),
        (
            SIG_FILE,
            descriptor("s", &[("SM3", "Doc_0/Document.xml")]).into_bytes(),
        ),
        ("Doc_0/Document.xml", b"y".to_vec()),
    ];
    let (mut sig, _pair) = make_signature(&entries, &[("SM3", "Doc_0/Document.xml")]);
    sig.certificate = None;
    let mut package = open(&entries);
    // Without a key nothing can be proved either way. Saying "tampered" would
    // accuse an untouched document, so the answer is that it was not checked.
    let out = verify_signature(&mut package, &sig).expect("no failure to report");
    assert_eq!(
        out,
        VerifyStatus::Unchecked,
        "a missing certificate leaves the signature unproved rather than broken"
    );
}

#[test]
fn digest_depends_on_reference_order() {
    let mk = |refs: &[(&str, &str)]| {
        let entries = vec![
            ("Signs/Signs.xml", signs_xml()),
            (SIG_FILE, descriptor("s", refs).into_bytes()),
            ("a.bin", vec![1u8; 32]),
            ("b.bin", vec![2u8; 32]),
        ];
        let mut package = open(&entries);
        let sig = read_signature(&mut package, SIG_FILE).expect("descriptor");
        signed_digest(&mut package, &sig)
            .expect("digest")
            .expect("SM3 is implemented")
    };
    let ab = mk(&[("SM3", "a.bin"), ("SM3", "b.bin")]);
    let ba = mk(&[("SM3", "b.bin"), ("SM3", "a.bin")]);
    assert_ne!(ab, ba, "order matters in the signed digest");
    assert_eq!(
        mk(&[("SM3", "a.bin"), ("SM3", "b.bin")]),
        ab,
        "digest is stable"
    );
}

#[test]
fn gb_t_33190_18_1_signature_list_points_at_sign_files() {
    let entries = vec![
        ("Signs/Signs.xml", signs_xml()),
        (
            SIG_FILE,
            descriptor("s", &[("SM3", "Doc_0/Document.xml")]).into_bytes(),
        ),
    ];
    let mut package = open(&entries);
    let signs = read_sign_list(&mut package, "Signs/Signs.xml").expect("list");
    assert_eq!(signs.len(), 1);
    assert_eq!(
        signs[0].base_loc, SIG_FILE,
        "each Sign names its descriptor file"
    );
    let sig = read_signature(&mut package, &signs[0].base_loc).expect("descriptor");
    assert_eq!(sig.references.len(), 1);
}

#[test]
fn gb_t_33190_18_2_1_digest_covers_every_reference() {
    let document = b"signed content".to_vec();
    let entries = vec![
        ("Signs/Signs.xml", signs_xml()),
        (
            SIG_FILE,
            descriptor("s", &[("SM3", "Doc_0/Document.xml")]).into_bytes(),
        ),
        ("Doc_0/Document.xml", document.clone()),
    ];
    let mut package = open(&entries);
    let sig = read_signature(&mut package, SIG_FILE).expect("descriptor");
    let first = signed_digest(&mut package, &sig)
        .expect("digest")
        .expect("SM3 is implemented");
    assert_eq!(first.len(), 32, "the SM3 digest is 32 bytes");

    let changed = vec![
        ("Signs/Signs.xml", signs_xml()),
        (
            SIG_FILE,
            descriptor("s", &[("SM3", "Doc_0/Document.xml")]).into_bytes(),
        ),
        ("Doc_0/Document.xml", b"edited content".to_vec()),
    ];
    let mut package = open(&changed);
    let second = signed_digest(&mut package, &sig)
        .expect("digest")
        .expect("SM3 is implemented");
    assert_ne!(
        first, second,
        "changing a referenced file changes the digest"
    );
}

#[test]
fn gb_t_33190_18_2_4_certificate_key_verifies_the_check_value() {
    let (bytes, _pair) = signed_fixture();
    let mut package = open_package_bytes(&bytes);
    let sig = read_signature(&mut package, SIGN_FILE).expect("descriptor");
    assert_eq!(
        verify_signature(&mut package, &sig).expect("verify"),
        VerifyStatus::Valid
    );
}

#[test]
fn gb_t_33190_18_2_4_tampered_content_fails_the_check_value() {
    let (mut extra, _key) = signed_descriptor_entries(
        "sig1",
        DOCUMENT_ENTRY,
        sofd_testkit::DOCUMENT_XML.as_bytes(),
    );
    let mut entries = sofd_testkit::base_entries(OFD_XML_SIGNED);
    entries.push((SIGNS_ENTRY, SIGNS_XML.as_bytes().to_vec()));
    let doc = entries.iter().position(|(n, _)| *n == DOCUMENT_ENTRY);
    entries[doc.expect("document entry")].1 = b"tampered".to_vec();
    entries.append(&mut extra);
    let mut package = open(&entries);
    let sig = read_signature(&mut package, SIGN_FILE).expect("descriptor");
    // The descriptor records each protected file's own digest, so the change is
    // caught there rather than only at the final signature check.
    let out = verify_signature(&mut package, &sig);
    assert!(
        matches!(out, Err(OfdError::Signature(_))),
        "tampered content must not verify, got {out:?}"
    );
}

#[test]
fn gb_t_33190_18_1_the_signature_list_carries_id_and_kind() {
    // Each Sign entry names its descriptor and says whether it is a seal or a
    // plain signature; Seal is what the schema defaults to.
    let xml = br#"<Signatures>
      <MaxSignId>S3</MaxSignId>
      <Signature ID="S1" BaseLoc="Signs/Sign_0.xml"/>
      <Signature ID="S2" Type="Sign" BaseLoc="Signs/Sign_1.xml"/>
      <Signature ID="S3" Type="Seal" BaseLoc="Signs/Sign_2.xml"/>
    </Signatures>"#
        .to_vec();
    let mut package = open(&[("Signs/Signs.xml", xml)]);
    let signs = read_sign_list(&mut package, "Signs/Signs.xml").expect("list");
    assert_eq!(signs.len(), 3);
    assert_eq!(signs[0].id.as_deref(), Some("S1"));
    assert_eq!(signs[0].kind, SignatureKind::Seal, "Seal is the default");
    assert_eq!(signs[1].kind, SignatureKind::Sign);
    assert_eq!(signs[2].kind, SignatureKind::Seal);
    assert_eq!(signs[2].base_loc, "Signs/Sign_2.xml");
}

#[test]
fn gb_t_33190_18_2_2_check_method_governs_every_reference() {
    // CheckMethod is written once on References and applies to each Reference
    // below it, so a descriptor with several protected files needs it only once.
    let xml = br#"<Signature ID="s">
      <SignedInfo><References CheckMethod="SM3">
        <Reference FileRef="a.bin"><CheckValue>AAAA</CheckValue></Reference>
        <Reference FileRef="b.bin"><CheckValue>BBBB</CheckValue></Reference>
      </References></SignedInfo>
    </Signature>"#
        .to_vec();
    let mut package = open(&[("Signs/Sign_0.xml", xml)]);
    let sig = read_signature(&mut package, "Signs/Sign_0.xml").expect("descriptor");
    assert_eq!(sig.references.len(), 2);
    assert!(sig.references.iter().all(|r| r.check_method == "SM3"));
    assert_eq!(sig.references[0].file_ref, "a.bin");
    assert_eq!(sig.references[1].file_ref, "b.bin");
    assert!(
        !sig.references[0].check_value.is_empty(),
        "each reference keeps its own recorded digest"
    );
    assert_ne!(
        sig.references[0].check_value, sig.references[1].check_value,
        "the two references do not share a digest"
    );
}

#[test]
fn gb_t_33190_18_2_3_the_proof_lives_behind_signed_value() {
    // SignedValue is a path, not the proof itself, so the descriptor alone is
    // not enough to verify: the referenced file has to be read too.
    let (bytes, pair) = signed_fixture();
    let mut package = open_package_bytes(&bytes);
    let sig = read_signature(&mut package, SIGN_FILE).expect("descriptor");
    assert!(
        sig.signed_value_ref.is_some(),
        "the descriptor points at its proof"
    );
    assert!(
        sig.check_value.is_empty(),
        "reading the descriptor alone leaves the proof unloaded"
    );

    // A key that is not the signer's must not verify it.
    let other = Sm2KeyPair::generate().expect("keygen");
    assert_eq!(
        verify_descriptor(&mut package, &sig, &other.public_bytes()).expect("verify"),
        VerifyStatus::Tampered,
        "a stranger's key does not validate the proof"
    );
    assert_eq!(
        verify_descriptor(&mut package, &sig, &pair.public_bytes()).expect("verify"),
        VerifyStatus::Valid,
        "the signer's key does"
    );
}

#[test]
fn gb_t_33190_18_2_3_a_signed_value_is_checked_over_its_own_content() {
    // GB/T 38540 puts the signed content in the envelope: the signature is over
    // the content SEQUENCE, tag and length included, so the bytes to check come
    // with the proof instead of being recomputed from the descriptor.
    let pair = Sm2KeyPair::generate().expect("keygen");
    let content = vec![0x30, 0x06, 0x02, 0x01, 0x04, 0x18, 0x01, 0x32];
    let signature = pair.sign(&content);
    let mut sig = Signature {
        signed_value_ref: Some("/Signs/Sign_0/SignedValue.dat".to_string()),
        descriptor_path: Some(SIG_FILE.to_string()),
        ..Default::default()
    };

    let mut body = content.clone();
    body.extend_from_slice(&[0x06, 0x01, 0x01]);
    body.push(0x03);
    body.push(signature.len() as u8 + 1);
    body.push(0x00);
    body.extend_from_slice(&signature);
    let mut envelope = vec![0x30, 0x81, body.len() as u8];
    envelope.extend_from_slice(&body);

    let mut package = open(&[
        ("Signs/Signs.xml", signs_xml()),
        (SIG_FILE, b"<Sign/>".to_vec()),
        ("Signs/Sign_0/SignedValue.dat", envelope),
    ]);
    assert_eq!(
        verify_descriptor(&mut package, &sig, &pair.public_bytes()).expect("verify"),
        VerifyStatus::Valid,
        "the content the envelope carries is what the signature covers"
    );

    // An envelope whose content moved no longer matches its own signature,
    // whatever the descriptor still says about it.
    sig.check_value = signature.clone();
    sig.signed_content = Some(vec![0x30, 0x06, 0x02, 0x01, 0x05, 0x18, 0x01, 0x32]);
    assert_eq!(
        verify_with_public_key(&mut package, &sig, &pair.public_bytes()).expect("verify"),
        VerifyStatus::Tampered,
        "edited content fails the same check the original passed"
    );
}

#[test]
fn gb_t_33190_18_2_3_a_stamp_records_where_it_sits() {
    // An electronic seal also says where on which page it is drawn, and may
    // clip itself to a smaller region.
    let xml = br#"<Signature ID="s">
      <SignedInfo><References CheckMethod="SM3">
        <Reference FileRef="a.bin"><CheckValue>AAAA</CheckValue></Reference>
      </References></SignedInfo>
      <StampAnnot ID="1" PageRef="3" Boundary="10 20 30 40" Clip="12 22 26 36"/>
      <Seal><BaseLoc>Res/Seal.esl</BaseLoc></Seal>
    </Signature>"#
        .to_vec();
    let mut package = open(&[("Signs/Sign_0.xml", xml)]);
    let sig = read_signature(&mut package, "Signs/Sign_0.xml").expect("descriptor");
    assert_eq!(sig.stamps.len(), 1);
    let stamp = &sig.stamps[0];
    assert_eq!(stamp.id, "1");
    assert_eq!(stamp.page_ref, "3");
    assert_eq!(
        stamp.boundary,
        sofd_common::Rect::new(10.0, 20.0, 30.0, 40.0)
    );
    assert_eq!(
        stamp.clip,
        Some(sofd_common::Rect::new(12.0, 22.0, 26.0, 36.0))
    );
    assert_eq!(sig.seal.as_deref(), Some("Res/Seal.esl"));
}

#[test]
fn gb_t_33190_18_2_2_the_provider_name_is_an_attribute() {
    // Provider carries its name as ProviderName rather than as element text,
    // which a text-only reader would silently lose.
    let xml = br#"<Signature ID="s">
      <SignedInfo>
        <Provider ProviderName="Example CA" Version="1.0"/>
        <SignatureMethod>SM2-SM3</SignatureMethod>
        <SignatureDateTime>2024-05-01T09:00:00</SignatureDateTime>
        <References CheckMethod="SM3">
          <Reference FileRef="a.bin"><CheckValue>AAAA</CheckValue></Reference>
        </References>
      </SignedInfo>
    </Signature>"#
        .to_vec();
    let mut package = open(&[("Signs/Sign_0.xml", xml)]);
    let sig = read_signature(&mut package, "Signs/Sign_0.xml").expect("descriptor");
    assert_eq!(sig.provider.as_deref(), Some("Example CA"));
    assert_eq!(sig.signature_method.as_deref(), Some("SM2-SM3"));
    assert_eq!(sig.sign_time.as_deref(), Some("2024-05-01T09:00:00"));
}

#[test]
fn gb_t_33190_18_2_1_a_sealed_document_verifies_and_an_edited_one_does_not() {
    // Clause 18.3: sealing fixes the content, so the same descriptor verifies
    // against what was sealed and reports a mismatch once that content moves.
    let sealed = vec![
        ("Signs/Signs.xml", signs_xml()),
        (
            SIG_FILE,
            descriptor("s", &[("SM3", "Doc_0/Document.xml")]).into_bytes(),
        ),
        ("Doc_0/Document.xml", b"the sealed document".to_vec()),
    ];
    let (sig, pair) = make_signature(&sealed, &[("SM3", "Doc_0/Document.xml")]);
    let mut package = open(&sealed);
    assert_eq!(
        verify_with_public_key(&mut package, &sig, &pair.public_bytes()).expect("verify"),
        VerifyStatus::Valid,
        "the sealed bytes verify"
    );

    let edited = vec![
        ("Signs/Signs.xml", signs_xml()),
        (
            SIG_FILE,
            descriptor("s", &[("SM3", "Doc_0/Document.xml")]).into_bytes(),
        ),
        ("Doc_0/Document.xml", b"the edited document".to_vec()),
    ];
    let mut package = open(&edited);
    assert_eq!(
        verify_with_public_key(&mut package, &sig, &pair.public_bytes()).expect("verify"),
        VerifyStatus::Tampered,
        "editing a sealed file breaks the seal"
    );
}

#[test]
fn gb_t_33190_18_2_3_validation_reports_the_scope_it_checked() {
    // Clause 18.4: validation covers exactly the files the descriptor names.
    // A file outside that list may change without the seal moving, and that
    // is the scope the check value was computed over.
    let entries = vec![
        ("Signs/Signs.xml", signs_xml()),
        (
            SIG_FILE,
            descriptor("s", &[("SM3", "Doc_0/Document.xml")]).into_bytes(),
        ),
        ("Doc_0/Document.xml", b"sealed".to_vec()),
        ("Doc_0/Res/Other.xml", b"outside the scope".to_vec()),
    ];
    let (sig, pair) = make_signature(&entries, &[("SM3", "Doc_0/Document.xml")]);
    let mut package = open(&entries);
    let read = read_signature(&mut package, SIG_FILE).expect("descriptor");
    assert_eq!(
        read.references
            .iter()
            .map(|r| r.file_ref.as_str())
            .collect::<Vec<_>>(),
        vec!["Doc_0/Document.xml"],
        "the descriptor states which file it covers"
    );

    // Editing a file outside the sealed set leaves the check value alone.
    let outside_edited = vec![
        ("Signs/Signs.xml", signs_xml()),
        (
            SIG_FILE,
            descriptor("s", &[("SM3", "Doc_0/Document.xml")]).into_bytes(),
        ),
        ("Doc_0/Document.xml", b"sealed".to_vec()),
        ("Doc_0/Res/Other.xml", b"edited outside the scope".to_vec()),
    ];
    let mut package = open(&outside_edited);
    assert_eq!(
        verify_with_public_key(&mut package, &sig, &pair.public_bytes()).expect("verify"),
        VerifyStatus::Valid,
        "the checked scope is the referenced files and nothing else"
    );
}

#[test]
fn gb_t_33190_18_2_3_a_seal_resolves_to_its_image_and_placements() {
    // The descriptor names the seal relative to its own file, so resolution
    // joins the two: every stamp comes back with the image bytes beside the
    // place it is drawn on.
    let xml = br#"<Signature ID="s">
      <SignedInfo><References CheckMethod="SM3">
        <Reference FileRef="a.bin"><CheckValue>AAAA</CheckValue></Reference>
      </References></SignedInfo>
      <StampAnnot ID="1" PageRef="3" Boundary="10 20 30 40"/>
      <StampAnnot ID="2" PageRef="4" Boundary="50 60 20 20"/>
      <Seal><BaseLoc>Res/Seal.esl</BaseLoc></Seal>
    </Signature>"#
        .to_vec();
    let image = b"seal-bytes".to_vec();
    let mut package = open(&[
        ("Signs/Sign_0.xml", xml),
        ("Signs/Res/Seal.esl", image.clone()),
    ]);
    let sig = read_signature(&mut package, "Signs/Sign_0.xml").expect("descriptor");
    let seals = read_seals(&mut package, "Signs/Sign_0.xml", &sig).expect("seals");
    assert_eq!(seals.len(), 2, "one placement per stamp");
    assert_eq!(seals[0].id, "1");
    assert_eq!(seals[0].page_ref, "3");
    assert_eq!(seals[0].image, image, "the seal image resolves beside it");
    assert_eq!(
        seals[1].boundary,
        sofd_common::Rect::new(50.0, 60.0, 20.0, 20.0)
    );
}

#[test]
fn gb_t_33190_18_2_3_a_seal_with_no_image_still_places() {
    // A descriptor may name a seal the package does not ship, or none at all;
    // the placement still travels so a reader can draw its placeholder rather
    // than lose the stamp.
    let xml = br#"<Signature ID="s">
      <SignedInfo><References CheckMethod="SM3">
        <Reference FileRef="a.bin"><CheckValue>AAAA</CheckValue></Reference>
      </References></SignedInfo>
      <StampAnnot ID="1" PageRef="3" Boundary="10 20 30 40"/>
      <Seal><BaseLoc>Res/Missing.esl</BaseLoc></Seal>
    </Signature>"#
        .to_vec();
    let mut package = open(&[("Signs/Sign_0.xml", xml)]);
    let sig = read_signature(&mut package, "Signs/Sign_0.xml").expect("descriptor");
    let seals = read_seals(&mut package, "Signs/Sign_0.xml", &sig).expect("seals");
    assert_eq!(seals.len(), 1);
    assert!(seals[0].image.is_empty(), "no bytes, but the place remains");
    assert_eq!(
        seals[0].image_ref.as_deref(),
        Some("Signs/Res/Missing.esl"),
        "the path is still reported"
    );
}

#[test]
fn gb_t_33190_18_2_2_coverage_names_what_a_signature_protects() {
    // Clause 18.2.2: a signature is scoped to the files its references name.
    // The report states that scope, and lists what the container holds that no
    // signature protects, which is the surface a later update would touch.
    let entries = vec![
        (
            "Signs/Signs.xml",
            br#"<Signs><Sign ID="s" BaseLoc="Signs/Sign_0.xml"/></Signs>"#.to_vec(),
        ),
        (
            SIG_FILE,
            descriptor("s", &[("SM3", "Doc_0/Document.xml")]).into_bytes(),
        ),
        ("Doc_0/Document.xml", b"sealed".to_vec()),
        ("Doc_0/Res/Font.ttf", b"a face".to_vec()),
    ];
    let mut package = open(&entries);
    let reports = read_coverage(&mut package, "Signs/Signs.xml").expect("coverage");
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].id.as_deref(), Some("s"));
    assert_eq!(reports[0].base_loc, SIG_FILE);
    assert_eq!(reports[0].covered, vec!["Doc_0/Document.xml"]);
    assert!(
        reports[0]
            .uncovered
            .contains(&"Doc_0/Res/Font.ttf".to_string()),
        "a file outside the scope is reported, got {:?}",
        reports[0].uncovered
    );
    assert!(
        !reports[0]
            .uncovered
            .contains(&"Doc_0/Document.xml".to_string()),
        "a covered file is not reported as exposed"
    );
}

#[test]
fn a_reference_that_starts_with_a_slash_protects_its_file() {
    // Issuers write `FileRef` with a leading slash and the container lists
    // without one. Comparing the spellings directly reported every referenced
    // file as also uncovered, which reads as a document that protects nothing.
    let entries = vec![
        (
            "Signs/Signs.xml",
            br#"<Signs><Sign ID="s" BaseLoc="Signs/Sign_0.xml"/></Signs>"#.to_vec(),
        ),
        (
            SIG_FILE,
            descriptor("s", &[("SM3", "/Doc_0/Document.xml")]).into_bytes(),
        ),
        ("Doc_0/Document.xml", b"sealed".to_vec()),
    ];
    let mut package = open(&entries);
    let reports = read_coverage(&mut package, "Signs/Signs.xml").expect("coverage");
    assert_eq!(reports[0].covered, vec!["/Doc_0/Document.xml"]);
    assert!(
        !reports[0]
            .uncovered
            .contains(&"Doc_0/Document.xml".to_string()),
        "the file the reference names is protected, got {:?}",
        reports[0].uncovered
    );
}

#[test]
fn gb_t_33190_18_2_2_a_later_signature_slides_under_the_earlier_one() {
    // The incremental-update property: adding a second signature leaves the
    // first one's scope intact, and a file the second one protects is no
    // longer reported as exposed by the union of both.
    let second = "Signs/Sign_1.xml";
    let list =
        br#"<Signs><Sign BaseLoc="Signs/Sign_0.xml"/><Sign BaseLoc="Signs/Sign_1.xml"/></Signs>"#
            .to_vec();
    let entries = vec![
        ("Signs/Signs.xml", list),
        (
            SIG_FILE,
            descriptor("first", &[("SM3", "Doc_0/Document.xml")]).into_bytes(),
        ),
        (
            second,
            descriptor("second", &[("SM3", "Doc_0/Pages/Page_1.xml")]).into_bytes(),
        ),
        ("Doc_0/Document.xml", b"sealed".to_vec()),
        ("Doc_0/Pages/Page_1.xml", b"added later".to_vec()),
    ];
    let mut package = open(&entries);
    let reports = read_coverage(&mut package, "Signs/Signs.xml").expect("coverage");
    assert_eq!(reports.len(), 2);
    assert_eq!(reports[0].covered, vec!["Doc_0/Document.xml"]);
    assert_eq!(reports[1].covered, vec!["Doc_0/Pages/Page_1.xml"]);
    assert!(
        reports[0].uncovered.is_empty(),
        "both content files are protected now, got {:?}",
        reports[0].uncovered
    );
}

/// A descriptor carrying a `<Timestamp>` element, the way a countersigned
/// signature writes one: the token is base64 DER beside the proof.
fn descriptor_with_timestamp(id: &str, refs: &[(&str, &str)], tok: &[u8]) -> String {
    let mut xml = format!(r#"<Signature ID="{id}"><SignedInfo>"#);
    if let Some((method, _)) = refs.first() {
        xml.push_str(&format!(r#"<References CheckMethod="{method}">"#));
    } else {
        xml.push_str("<References>");
    }
    for (_, file) in refs {
        xml.push_str(&format!(
            r#"<Reference FileRef="{file}"><CheckValue></CheckValue></Reference>"#
        ));
    }
    xml.push_str("</References></SignedInfo>");
    xml.push_str(&format!(r#"<Timestamp>{}</Timestamp>"#, base64_of(tok)));
    xml.push_str("</Signature>");
    xml
}

#[test]
fn a_descriptor_timestamp_reads_as_a_token() {
    let imprint = sofd_crypto::sm3(b"the signature value").to_vec();
    let (token, _tsa) = timestamp_token(&TimestampSpec {
        imprint: imprint.clone(),
        gen_time: "20260101000000Z".into(),
        serial: 5,
    });
    let entries = vec![
        ("Signs/Signs.xml", signs_xml()),
        (
            SIG_FILE,
            descriptor_with_timestamp("s", &[("SM3", DOCUMENT_ENTRY)], &token).into_bytes(),
        ),
    ];
    let mut package = open(&entries);
    let sig = read_signature(&mut package, SIG_FILE).expect("descriptor");
    let parsed = sig
        .timestamp_token()
        .expect("the timestamp parses")
        .expect("a token is present");
    assert_eq!(parsed.gen_time, "20260101000000Z");
    assert_eq!(parsed.serial_number, "5");
    assert_eq!(parsed.imprint, imprint);
}

#[test]
fn a_signature_without_a_timestamp_says_so() {
    let entries = vec![
        ("Signs/Signs.xml", signs_xml()),
        (
            SIG_FILE,
            descriptor("s", &[("SM3", DOCUMENT_ENTRY)]).into_bytes(),
        ),
    ];
    let mut package = open(&entries);
    let sig = read_signature(&mut package, SIG_FILE).expect("descriptor");
    assert!(sig.timestamp.is_none());
    assert_eq!(
        verify_timestamp(&sig).expect("no token is not an error"),
        TimestampStatus::Absent
    );
}

#[test]
fn gb_t_33190_18_2_4_a_timestamp_bound_to_the_signature_verifies() {
    let mut sig = Signature {
        check_value: b"the signature value".to_vec(),
        ..Signature::default()
    };
    let (token, _tsa) = timestamp_token(&TimestampSpec::over(&sig.check_value));
    sig.timestamp = Some(token);
    assert_eq!(
        verify_timestamp(&sig).expect("token verifies"),
        TimestampStatus::Bound,
        "the imprint is this signature's digest, so the stamp is its own"
    );
}

#[test]
fn gb_t_33190_18_2_4_a_timestamp_over_other_bytes_is_unbound() {
    let mut sig = Signature {
        check_value: b"the signature value".to_vec(),
        ..Signature::default()
    };
    let (token, _tsa) = timestamp_token(&TimestampSpec::over(b"a different signature value"));
    sig.timestamp = Some(token);
    assert_eq!(
        verify_timestamp(&sig).expect("token parses"),
        TimestampStatus::Unbound,
        "a stamp over another value must not read as this signature's"
    );
}

#[test]
fn gb_t_33190_18_2_4_a_timestamp_whose_proof_is_wrong_is_unverified() {
    let mut sig = Signature {
        check_value: b"the signature value".to_vec(),
        ..Signature::default()
    };
    let (mut token, _tsa) = timestamp_token(&TimestampSpec::over(&sig.check_value));
    // Flip the last byte of the token, which is inside the TSA signature.
    if let Some(last) = token.last_mut() {
        *last ^= 0xff;
    }
    sig.timestamp = Some(token);
    let status = verify_timestamp(&sig).expect("a broken proof still parses");
    assert_ne!(
        status,
        TimestampStatus::Bound,
        "a tampered token must not read as bound"
    );
}

/// Apply `edit` to the fixture's descriptor text, leaving every other entry.
fn resign_descriptor(bytes: &[u8], edit: impl Fn(&str) -> String) -> String {
    let mut package = open_package_bytes(bytes);
    let descriptor = package.read_string(SIGN_FILE).expect("descriptor");
    edit(&descriptor)
}

/// Rebuild an archive with one entry's bytes replaced, keeping every other.
fn swap_entry(bytes: &[u8], path: &str, replacement: &[u8]) -> Vec<u8> {
    let mut package = open_package_bytes(bytes);
    let names: Vec<String> = package.entries().to_vec();
    let mut entries: Vec<(String, Vec<u8>)> = Vec::with_capacity(names.len());
    for name in &names {
        let data = package.read(name).expect("entry reads");
        entries.push((name.clone(), data));
    }
    for (name, data) in &mut entries {
        if name == path {
            *data = replacement.to_vec();
        }
    }
    let borrowed: Vec<(&str, &[u8])> = entries
        .iter()
        .map(|(name, data)| (name.as_str(), data.as_slice()))
        .collect();
    sofd_package::build_archive(&borrowed).expect("archive")
}

#[test]
fn gb_t_33190_18_2_4_a_digest_this_build_cannot_compute_is_unchecked() {
    // The descriptor may name a digest this build does not implement. Nothing
    // about the content is in doubt, so a verdict of tampered would be a claim
    // nobody made; the signature is reported as unchecked instead.
    let entries = vec![
        ("Signs/Signs.xml", signs_xml()),
        (
            SIG_FILE,
            descriptor("s", &[("SHA256", "Doc_0/Document.xml")]).into_bytes(),
        ),
        ("Doc_0/Document.xml", b"x".to_vec()),
    ];
    let sig = read_signature(&mut open(&entries), SIG_FILE).expect("descriptor");
    let pair = Sm2KeyPair::generate().expect("keygen");
    let mut package = open(&entries);
    assert_eq!(
        verify_with_public_key(&mut package, &sig, &pair.public_bytes()).expect("verdict"),
        VerifyStatus::Unchecked,
        "an algorithm this build lacks is not evidence of an edit"
    );
}

#[test]
fn gb_t_33190_18_2_4_a_signature_that_does_not_add_up_is_tampered() {
    // The proof is intact and the digest runs, so a mismatch here is the
    // content having moved, which is the one thing that is a claim.
    let (bytes, pair) = signed_fixture();
    let mut package = open_package_bytes(&bytes);
    let mut sig = read_signature(&mut package, SIGN_FILE).expect("descriptor");
    sig.read_check_value(&mut package).expect("proof");
    sig.check_value[8] ^= 0xff;
    assert_eq!(
        verify_with_public_key(&mut package, &sig, &pair.public_bytes()).expect("verdict"),
        VerifyStatus::Tampered
    );
}

#[test]
fn gb_t_33190_18_2_4_a_certificate_past_its_window_is_unchecked() {
    // An expired key proves possession and nothing else, since anyone could
    // have used it. GB/T 38540-2020 clause 7.2 judges it at the signing time,
    // so the descriptor's SignatureDateTime is what the window is read against.
    let (bytes, pair) = signed_fixture();
    let expired = base64_of(&sm2_cert_der_over(
        &pair.public_bytes(),
        "180101000000Z",
        "190101000000Z",
    ));
    let dated = |descriptor: &str| {
        let with_cert = replace_element(descriptor, "Cert", &expired);
        let with_time = with_cert.replace(
            "<SignedValue>",
            "<SignatureDateTime>2026-01-01T00:00:00Z</SignatureDateTime><SignedValue>",
        );
        assert!(with_time.contains("2026-01-01"), "the date reaches the XML");
        with_time
    };
    let path = swap_entry(
        &bytes,
        SIGN_FILE,
        resign_descriptor(&bytes, dated).as_bytes(),
    );
    let mut package = open_package_bytes(&path);
    let sig = read_signature(&mut package, SIGN_FILE).expect("descriptor");
    assert_eq!(
        verify_signature(&mut package, &sig).expect("verdict"),
        VerifyStatus::Unchecked,
        "an expired certificate is not a match and not an edit either"
    );
}

/// Replace the text of the first `Cert` element in a descriptor.
fn replace_element(xml: &str, element: &str, value: &str) -> String {
    let open = format!("<{element}>");
    let close = format!("</{element}>");
    let Some(start) = xml.find(&open) else {
        return xml.to_string();
    };
    let from = start + open.len();
    let Some(end) = xml[from..].find(&close) else {
        return xml.to_string();
    };
    format!("{}{}{}", &xml[..from], value, &xml[from + end..])
}

// The issuer question, GB/T 38540-2020 7.3 e) 1). The anchor is an argument
// rather than a list the engine ships, so these tests pass one in.

use sofd_crypto::{Certificate, TrustAnchors};
use sofd_sign::{TrustPolicy, verify_signature_under};
use sofd_testkit::SIGN_FILE as CHAIN_SIG;
use sofd_testkit::{cert_der, chain, signed_fixture_over, signed_fixture_with};

fn verify_under_chain(
    der: &[u8],
    anchors: &TrustAnchors,
    intermediates: &[Certificate],
) -> VerifyStatus {
    let mut package = open_package_bytes(der);
    let sig = read_signature(&mut package, CHAIN_SIG).expect("descriptor");
    verify_signature_under(
        &mut package,
        &sig,
        &TrustPolicy::Issuer {
            anchors,
            intermediates,
        },
    )
    .expect("verification runs")
}

#[test]
fn a_signer_that_chains_to_the_anchor_is_valid() {
    let chain = chain();
    let mut anchors = TrustAnchors::none();
    anchors.add(&chain.root_der).expect("root anchor");
    let intermediate = Certificate::parse_der(&chain.intermediate_der).expect("intermediate");
    let der = signed_fixture_with(&chain.leaf_der, &chain.leaf_key);
    assert_eq!(
        verify_under_chain(&der, &anchors, &[intermediate]),
        VerifyStatus::Valid
    );
}

#[test]
fn a_signer_outside_the_named_anchors_is_untrusted() {
    let chain = chain();
    // The same document, but the caller only trusts some other root.
    let other = chain_with_other_root();
    let mut anchors = TrustAnchors::none();
    anchors.add(&other).expect("other anchor");
    let intermediate = Certificate::parse_der(&chain.intermediate_der).expect("intermediate");
    let der = signed_fixture_with(&chain.leaf_der, &chain.leaf_key);
    assert_eq!(
        verify_under_chain(&der, &anchors, &[intermediate]),
        VerifyStatus::Untrusted
    );
}

#[test]
fn an_empty_anchor_set_makes_every_signer_untrusted() {
    let chain = chain();
    let intermediate = Certificate::parse_der(&chain.intermediate_der).expect("intermediate");
    let der = signed_fixture_with(&chain.leaf_der, &chain.leaf_key);
    assert_eq!(
        verify_under_chain(&der, &TrustAnchors::none(), &[intermediate]),
        VerifyStatus::Untrusted
    );
}

#[test]
fn the_proof_policy_does_not_look_at_the_issuer() {
    let chain = chain();
    let der = signed_fixture_with(&chain.leaf_der, &chain.leaf_key);
    let mut package = open_package_bytes(&der);
    let sig = read_signature(&mut package, CHAIN_SIG).expect("descriptor");
    assert_eq!(
        verify_signature_under(&mut package, &sig, &TrustPolicy::Proof).expect("verification"),
        VerifyStatus::Valid
    );
}

#[test]
fn a_tampered_document_is_tampered_even_when_untrusted() {
    let chain = chain();
    // Signed over one text, then rebuilt over another. An incident outranks a
    // policy question, so the verdict must be tampered and not untrusted.
    let signed = signed_fixture_with(&chain.leaf_der, &chain.leaf_key);
    let edited = signed_fixture_over(&chain.leaf_der, &chain.leaf_key, b"the edited document");
    assert_ne!(signed, edited, "the two documents differ");

    let mut package = open_package_bytes(&edited);
    let sig = read_signature(&mut package, CHAIN_SIG).expect("descriptor");
    // An edited document is reported before the path is even built: the stored
    // per-file digest already disagrees, which is a fact about the document and
    // not about who signed it.
    let err = verify_signature_under(
        &mut package,
        &sig,
        &TrustPolicy::Issuer {
            anchors: &TrustAnchors::none(),
            intermediates: &[],
        },
    )
    .expect_err("the stored digest no longer matches");
    assert!(
        matches!(err, OfdError::Signature(SignatureError::Tampered)),
        "{err:?}"
    );
}

fn chain_with_other_root() -> Vec<u8> {
    use sofd_crypto::Sm2KeyPair;
    use sofd_testkit::{BasicConstraints, CertSpec, KeyUsageBits};
    let key = Sm2KeyPair::generate().expect("key");
    let mut spec = CertSpec::ca("Other Root", "Other Root", 77);
    spec.subject_key_id = Some(sofd_testkit::key_id(&key));
    spec.basic_constraints = Some(BasicConstraints {
        ca: true,
        path_len: None,
    });
    spec.key_usage = Some(KeyUsageBits::for_ca());
    cert_der(&spec, &key.public_bytes(), &key)
}
