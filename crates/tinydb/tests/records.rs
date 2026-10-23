use std::io::{self, ErrorKind};

use tinydb::storage::{MAX_RECORD_SIZE, PAGE_SIZE, Record, SlottedPage, Value};

fn assert_error<T>(result: io::Result<T>, expected: ErrorKind) {
    match result {
        Ok(_) => panic!("expected {expected:?}, got success"),
        Err(error) => assert_eq!(error.kind(), expected, "{error}"),
    }
}

#[test]
fn record_encoding_has_explicit_tags_lengths_and_little_endian_numbers() {
    let record = Record::new(vec![
        Value::Null,
        Value::Integer(-2),
        Value::Text("é".into()),
    ]);
    let expected = vec![
        3, 0, // three fields
        0, // null
        1, 254, 255, 255, 255, 255, 255, 255, 255, // integer -2
        2, 2, 0, 0, 0, 195, 169, // two UTF-8 bytes
    ];
    assert_eq!(record.encode().unwrap(), expected);
    assert_eq!(Record::decode(&expected).unwrap(), record);
}

#[test]
fn records_round_trip_empty_values_unicode_and_integer_extremes() {
    for values in [
        vec![],
        vec![Value::Null, Value::Text(String::new())],
        vec![Value::Integer(i64::MIN), Value::Integer(i64::MAX)],
        vec![Value::Text("Hola 🌍\0数据库".into()), Value::Integer(0)],
    ] {
        let record = Record::new(values);
        assert_eq!(Record::decode(&record.encode().unwrap()).unwrap(), record);
    }
}

#[test]
fn codec_rejects_truncation_unknown_tags_bad_utf8_and_trailing_bytes() {
    let encoded = Record::new(vec![Value::Text("hello".into()), Value::Integer(42)])
        .encode()
        .unwrap();
    for len in 0..encoded.len() {
        assert_error(Record::decode(&encoded[..len]), ErrorKind::InvalidData);
    }
    for bytes in [
        vec![1, 0, 3],                     // unknown tag
        vec![1, 0, 2, 1, 0, 0, 0, 255],    // invalid UTF-8
        vec![1, 0, 2, 255, 255, 255, 255], // impossible text length
        vec![0, 0, 0],                     // trailing bytes
        vec![255, 255],                    // field count exceeds available tags
    ] {
        assert_error(Record::decode(&bytes), ErrorKind::InvalidData);
    }
}

#[test]
fn codec_rejects_an_unrepresentable_field_count() {
    let record = Record::new(vec![Value::Null; usize::from(u16::MAX) + 1]);
    assert_error(record.encode(), ErrorKind::InvalidInput);
}

#[test]
fn slotted_page_bytes_match_the_documented_layout() {
    let mut page = SlottedPage::new();
    assert_eq!(page.free_space(), PAGE_SIZE - 16);
    assert_eq!(page.insert(b"abc").unwrap(), 0);
    let bytes = page.as_page();
    assert_eq!(&bytes[..8], &[b'S', b'L', b'T', b'1', 1, 0, 253, 15]);
    assert!(bytes[8..16].iter().all(|&b| b == 0));
    assert_eq!(&bytes[16..20], &[253, 15, 3, 0]);
    assert!(bytes[20..PAGE_SIZE - 3].iter().all(|&b| b == 0));
    assert_eq!(&bytes[PAGE_SIZE - 3..], b"abc");
    let page = SlottedPage::from_page(*bytes).unwrap();
    assert_eq!(page.read(0).unwrap(), b"abc");
    assert_eq!(page.free_space(), PAGE_SIZE - 23);
}

#[test]
fn maximum_payload_fits_and_capacity_errors_preserve_original_bytes() {
    let mut page = SlottedPage::new();
    let payload = vec![7; MAX_RECORD_SIZE];
    assert_eq!(page.insert(&payload).unwrap(), 0);
    assert_eq!(page.free_space(), 0);
    let before = *page.as_page();
    assert_error(page.insert(b"x"), ErrorKind::StorageFull);
    assert_error(
        page.update(0, &[1; MAX_RECORD_SIZE + 1]),
        ErrorKind::StorageFull,
    );
    assert_eq!(page.as_page(), &before);
    assert_eq!(page.read(0).unwrap(), payload);

    page.update(0, b"small").unwrap();
    assert_eq!(page.insert(b"another").unwrap(), 1);
    assert_eq!(page.read(0).unwrap(), b"small");
}

#[test]
fn tombstones_keep_ids_stable_while_payload_space_is_reclaimed() {
    let mut page = SlottedPage::new();
    let ids = [
        page.insert(&[1; 1000]).unwrap(),
        page.insert(&[2; 1000]).unwrap(),
        page.insert(&[3; 1000]).unwrap(),
    ];
    page.delete(ids[1]).unwrap();
    assert_error(page.read(ids[1]), ErrorKind::NotFound);
    let new_id = page.insert(&[4; 1500]).unwrap();
    assert_eq!(new_id, 3);
    assert_eq!(page.read(ids[0]).unwrap(), &[1; 1000]);
    assert_eq!(page.read(ids[2]).unwrap(), &[3; 1000]);
    page.update(ids[0], &[5; 1200]).unwrap();
    assert_eq!(page.read(ids[0]).unwrap(), &[5; 1200]);
    assert_eq!(page.read(ids[2]).unwrap(), &[3; 1000]);
    assert_eq!(page.read(new_id).unwrap(), &[4; 1500]);
    let page = SlottedPage::from_page(*page.as_page()).unwrap();
    assert_eq!(page.slot_count(), 4);
    assert_error(page.read(ids[1]), ErrorKind::NotFound);
}

#[test]
fn invalid_slots_empty_payloads_and_failed_growth_do_not_modify_the_page() {
    let mut page = SlottedPage::new();
    page.insert(&[1; 2000]).unwrap();
    page.insert(&[2; 2000]).unwrap();
    let before = *page.as_page();
    assert_error(page.update(0, &[3; 2100]), ErrorKind::StorageFull);
    assert_error(page.insert(b""), ErrorKind::InvalidInput);
    assert_error(page.update(0, b""), ErrorKind::InvalidInput);
    for slot in [2, u16::MAX] {
        assert_error(page.read(slot), ErrorKind::NotFound);
        assert_error(page.delete(slot), ErrorKind::NotFound);
        assert_error(page.update(slot, b"x"), ErrorKind::NotFound);
    }
    assert_eq!(page.as_page(), &before);
    page.delete(1).unwrap();
    let before = *page.as_page();
    assert_error(page.delete(1), ErrorKind::NotFound);
    assert_error(page.update(1, b"x"), ErrorKind::NotFound);
    assert_eq!(page.as_page(), &before);
    page.update(0, &[3; 2100]).unwrap();
    assert_eq!(page.read(0).unwrap(), &[3; 2100]);
}

#[test]
fn tombstones_exhaust_the_directory_without_reusing_deleted_ids() {
    let mut page = SlottedPage::new();
    // 1019 tombstones leave four bytes: enough for one slot but no payload.
    for slot in 0..1019 {
        assert_eq!(page.insert(b"x").unwrap(), slot);
        page.delete(slot).unwrap();
    }
    assert_eq!(page.slot_count(), 1019);
    assert_eq!(page.free_space(), 4);
    let before = *page.as_page();
    assert_error(page.insert(b"x"), ErrorKind::StorageFull);
    assert_eq!(page.as_page(), &before);
    assert!(SlottedPage::from_page(before).is_ok());
}

#[test]
fn rejects_corrupt_slotted_page_headers_directories_and_payload_ranges() {
    let mut page = SlottedPage::new();
    page.insert(b"abc").unwrap();
    page.insert(b"def").unwrap();
    let valid = *page.as_page();
    let mut cases = vec![[0; PAGE_SIZE]];
    for (offset, bytes) in [
        (0, b"SLT2".to_vec()),
        (8, vec![1]),                          // reserved byte
        (4, u16::MAX.to_le_bytes().to_vec()),  // oversized directory
        (6, 4097_u16.to_le_bytes().to_vec()),  // data past page end
        (6, 16_u16.to_le_bytes().to_vec()),    // directory overlaps data
        (16, 1_u16.to_le_bytes().to_vec()),    // payload overlaps header
        (16, 4095_u16.to_le_bytes().to_vec()), // payload extends past end
        (18, 0_u16.to_le_bytes().to_vec()),    // malformed tombstone
        (20, valid[16..18].to_vec()),          // overlapping live payloads
        (24, vec![1]),                         // nonzero free space
    ] {
        let mut bytes_to_corrupt = valid;
        bytes_to_corrupt[offset..offset + bytes.len()].copy_from_slice(&bytes);
        cases.push(bytes_to_corrupt);
    }
    // A shortened payload leaves a gap between records.
    let mut gap = valid;
    gap[22..24].copy_from_slice(&2_u16.to_le_bytes());
    cases.push(gap);
    for bytes in cases {
        assert_error(SlottedPage::from_page(bytes), ErrorKind::InvalidData);
    }
}

#[test]
fn mixed_mutations_match_a_reference_model_and_survive_serialization() {
    let mut page = SlottedPage::new();
    let mut model: Vec<Option<Vec<u8>>> = Vec::new();
    let mut random = 7_u64;
    for step in 0..1500 {
        random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
        let payload = vec![(step % 256) as u8; ((random >> 32) % 150 + 1) as usize];
        let before = *page.as_page();
        if model.is_empty() || random.is_multiple_of(3) {
            let fits = payload.len() + 4 <= page.free_space();
            let result = page.insert(&payload);
            if fits {
                assert_eq!(usize::from(result.unwrap()), model.len());
                model.push(Some(payload));
            } else {
                assert_error(result, ErrorKind::StorageFull);
                assert_eq!(page.as_page(), &before);
            }
        } else {
            let slot = (random >> 16) as usize % model.len();
            if random % 3 == 1 {
                let result = page.delete(slot as u16);
                if model[slot].is_some() {
                    result.unwrap();
                    model[slot] = None;
                } else {
                    assert_error(result, ErrorKind::NotFound);
                }
            } else {
                let result = page.update(slot as u16, &payload);
                if let Some(old) = &model[slot] {
                    let used: usize = model.iter().flatten().map(Vec::len).sum();
                    let free = PAGE_SIZE - 16 - 4 * model.len() - used;
                    if payload.len() <= free + old.len() {
                        result.unwrap();
                        model[slot] = Some(payload);
                    } else {
                        assert_error(result, ErrorKind::StorageFull);
                        assert_eq!(page.as_page(), &before);
                    }
                } else {
                    assert_error(result, ErrorKind::NotFound);
                }
            }
        }
        page = SlottedPage::from_page(*page.as_page()).unwrap();
        assert_eq!(usize::from(page.slot_count()), model.len());
        let used: usize = model.iter().flatten().map(Vec::len).sum();
        assert_eq!(page.free_space(), PAGE_SIZE - 16 - model.len() * 4 - used);
        for (slot, expected) in model.iter().enumerate() {
            match expected {
                Some(bytes) => assert_eq!(page.read(slot as u16).unwrap(), bytes),
                None => assert_error(page.read(slot as u16), ErrorKind::NotFound),
            }
        }
    }
}
