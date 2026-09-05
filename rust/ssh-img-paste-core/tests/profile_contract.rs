use ssh_img_paste_core::ProfileId;

#[test]
fn accepts_contract_profile_id() {
    let id = ProfileId::parse("work_ssh-2").expect("valid profile id");
    assert_eq!(id.as_str(), "work_ssh-2");
}

#[test]
fn rejects_traversal_and_non_ascii_profile_ids() {
    for value in ["", "../escape", "-option", "work/other", "wörk"] {
        assert!(ProfileId::parse(value).is_err(), "accepted {value:?}");
    }
}
