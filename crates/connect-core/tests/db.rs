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

#[test]
fn anniversary_project_and_render() {
    let (dir, db) = temp_db();
    let input = connect_core::ProjectInput {
        name: "纪念日".to_string(),
        event_date: "2026-11-15".to_string(),
        title: "给未来的我们".to_string(),
        recipient: "小满".to_string(),
        opening: "开场".to_string(),
        body: "第一段\n第二段".to_string(),
        closing: "结尾".to_string(),
        template_id: "starry".to_string(),
        theme: "#6ea8ff".to_string(),
        music: String::new(),
        video: String::new(),
        custom_css: String::new(),
        photo_focus: Default::default(),
        slideshow: false,
    };

    let project = db.create_project(&input).unwrap();
    assert_eq!(project.slug.len(), 8);
    assert_eq!(project.photos.len(), 0);

    let loaded = db.get_project(project.id).unwrap().unwrap();
    let html = connect_core::anniversary::render_project(&loaded, "photos/", "media/");
    assert!(html.contains("给未来的我们"));
    assert!(html.contains("小满"));
    assert!(html.contains("#6ea8ff"));
    assert!(html.contains("第一段"));

    // 裁剪焦点随项目保存
    let mut input = connect_core::ProjectInput {
        name: loaded.name.clone(),
        event_date: loaded.event_date.clone(),
        title: loaded.title.clone(),
        recipient: loaded.recipient.clone(),
        opening: loaded.opening.clone(),
        body: loaded.body.clone(),
        closing: loaded.closing.clone(),
        template_id: loaded.template_id.clone(),
        theme: loaded.theme.clone(),
        music: loaded.music.clone(),
        video: loaded.video.clone(),
        custom_css: loaded.custom_css.clone(),
        photo_focus: Default::default(),
        slideshow: false,
    };
    input.photo_focus.insert("a.jpg".to_string(), [30.0, 70.0]);
    db.update_project(project.id, &input).unwrap();
    let reloaded = db.get_project(project.id).unwrap().unwrap();
    assert_eq!(reloaded.photo_focus.get("a.jpg"), Some(&[30.0, 70.0]));

    let version = db.add_project_version(project.id, "https://example.com/a", "s3").unwrap();
    assert_eq!(version.version, 1);
    assert_eq!(db.list_project_versions(project.id).unwrap().len(), 1);

    db.delete_project(project.id).unwrap();
    assert!(db.get_project(project.id).unwrap().is_none());

    let _ = std::fs::remove_dir_all(dir);
}
