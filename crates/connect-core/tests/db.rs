use connect_core::db::Database;
use connect_core::server::tag_payload;

fn temp_db() -> (std::path::PathBuf, Database) {
    let dir = std::env::temp_dir().join(format!("connect-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let db = Database::open(&dir.join("connect.db")).unwrap();
    (dir, db)
}

#[test]
fn trigger_crud_and_tag_payload() {
    let (dir, db) = temp_db();

    let trigger = db
        .create_trigger("测试触发器", "message", "{}")
        .expect("create");
    assert_eq!(trigger.name, "测试触发器");
    assert!(trigger.enabled);
    assert_eq!(trigger.public_id.len(), 8);

    let loaded = db.find_by_id(trigger.id).unwrap().expect("find");
    assert_eq!(loaded.token.len(), 32);
    assert_eq!(loaded.write_token.len(), 32);

    db.update_trigger(trigger.id, "改名", false, "message", "{}")
        .unwrap();
    let updated = db.get_trigger(trigger.id).unwrap().unwrap();
    assert_eq!(updated.name, "改名");
    assert!(!updated.enabled);

    let payload = tag_payload(&db, trigger.id, 53400).unwrap();
    assert!(payload.uri.contains("/t/"));
    assert!(payload.uri.contains("?k="));
    assert!(payload.write_page.contains("/w/"));
    assert!(payload.bytes < 888);

    db.mark_tag_written(trigger.id).unwrap();
    assert!(db.find_by_id(trigger.id).unwrap().unwrap().tag_written);

    let run = db.start_run(trigger.id, "test").unwrap();
    db.finish_run(run, "success", None).unwrap();
    assert_eq!(db.list_runs(10).unwrap()[0].status, "success");

    db.delete_trigger(trigger.id).unwrap();
    assert!(db.find_by_id(trigger.id).unwrap().is_none());

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn settings_round_trip() {
    let (dir, db) = temp_db();
    let mut settings = db.get_settings().unwrap();
    assert_eq!(settings.port, 53400);

    settings.device_name = "NFC Desk".to_string();
    settings.port = 53401;
    settings.auto_start = true;
    db.save_settings(&settings).unwrap();

    let loaded = db.get_settings().unwrap();
    assert_eq!(loaded.device_name, "NFC Desk");
    assert_eq!(loaded.port, 53401);
    assert!(loaded.auto_start);

    let _ = std::fs::remove_dir_all(dir);
}
