CREATE TABLE collection(id TEXT PRIMARY KEY, created_at TEXT, app_version TEXT);

CREATE TABLE settings(key TEXT PRIMARY KEY, value TEXT);

CREATE TABLE photos(id INTEGER PRIMARY KEY AUTOINCREMENT, identity_key TEXT UNIQUE,
                status TEXT NOT NULL, created_scan INTEGER);

CREATE TABLE files(id INTEGER PRIMARY KEY AUTOINCREMENT,
                relpath TEXT NOT NULL UNIQUE, relpath_fold TEXT NOT NULL, dir TEXT NOT NULL,
                name TEXT NOT NULL, stem TEXT NOT NULL, ext TEXT NOT NULL, size INTEGER NOT NULL,
                mtime_ns INTEGER NOT NULL, inode TEXT, quick_hash TEXT, full_hash TEXT,
                photo_id INTEGER, status TEXT NOT NULL, first_seen_scan INTEGER,
                last_seen_scan INTEGER, meta_state TEXT NOT NULL DEFAULT 'pending', meta_error TEXT);

CREATE TABLE meta(file_id INTEGER PRIMARY KEY, make TEXT, model TEXT, serial TEXT,
                shutter_count INTEGER, dt_original TEXT, subsec TEXT, offset_time TEXT,
                width INTEGER, height INTEGER, orientation INTEGER, lens TEXT, focal REAL,
                aperture REAL, exposure TEXT, iso INTEGER, lat REAL, lon REAL, alt REAL,
                rating INTEGER, label TEXT, json TEXT);

CREATE TABLE scans(id INTEGER PRIMARY KEY AUTOINCREMENT, kind TEXT, scope TEXT,
                started_at TEXT, finished_at TEXT, status TEXT, stats_json TEXT);

CREATE TABLE changes(id INTEGER PRIMARY KEY AUTOINCREMENT, scan_id INTEGER,
                kind TEXT, file_id INTEGER, photo_id INTEGER, old_relpath TEXT, new_relpath TEXT, at TEXT);

CREATE TABLE face_settings(key TEXT PRIMARY KEY, value TEXT NOT NULL);

CREATE TABLE persons(id INTEGER PRIMARY KEY AUTOINCREMENT,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP, merged_into INTEGER);

CREATE TABLE person_attrs(person_id INTEGER, key TEXT, value TEXT,
            PRIMARY KEY(person_id,key));

CREATE TABLE faces(id INTEGER PRIMARY KEY AUTOINCREMENT, photo_id INTEGER NOT NULL,
            source_file_id INTEGER, source_relpath TEXT NOT NULL,
            detector_id TEXT NOT NULL, bbox_x1 REAL, bbox_y1 REAL, bbox_x2 REAL, bbox_y2 REAL,
            landmarks TEXT, det_score REAL, quality REAL, person_id INTEGER,
            assignment TEXT CHECK(assignment IN ('user','auto','suggested') OR assignment IS NULL),
            confidence REAL, assigned_at TEXT, ignored INTEGER NOT NULL DEFAULT 0,
            retained INTEGER NOT NULL DEFAULT 0, captured_at TEXT, source_fingerprint TEXT);

CREATE TABLE face_embeddings(face_id INTEGER, embedder_id TEXT, dim INTEGER, vec BLOB,
            PRIMARY KEY(face_id,embedder_id));

CREATE TABLE face_rejections(face_id INTEGER, person_id INTEGER,
            PRIMARY KEY(face_id,person_id));

CREATE TABLE training_set(person_id INTEGER, face_id INTEGER, embedder_id TEXT,
            split TEXT CHECK(split IN ('ref','val')), selected_at TEXT, reason TEXT,
            pinned INTEGER NOT NULL DEFAULT 0, PRIMARY KEY(person_id,face_id,embedder_id));

CREATE TABLE face_review(face_id INTEGER, embedder_id TEXT, reason TEXT,
            PRIMARY KEY(face_id,embedder_id));

CREATE TABLE relation_types(id INTEGER PRIMARY KEY, name TEXT UNIQUE,
            inverse_id INTEGER, symmetric INTEGER);

CREATE TABLE relations(from_person INTEGER, type_id INTEGER, to_person INTEGER,
            PRIMARY KEY(from_person,type_id,to_person));

CREATE TABLE face_jobs(photo_id INTEGER, stage TEXT, model_id TEXT, status TEXT,
            error TEXT, updated_at TEXT, fingerprint TEXT,
            PRIMARY KEY(photo_id,stage,model_id));

CREATE TABLE training_exclusions(face_id INTEGER, embedder_id TEXT,
                PRIMARY KEY(face_id,embedder_id));

CREATE TABLE face_group_catalog(group_id INTEGER PRIMARY KEY AUTOINCREMENT,
            embedder_id TEXT NOT NULL,signature INTEGER NOT NULL,representative BLOB NOT NULL,
            members INTEGER NOT NULL DEFAULT 0);

CREATE TABLE face_group_dirty(embedder_id TEXT,face_id INTEGER,PRIMARY KEY(embedder_id,face_id));

CREATE TABLE face_group_members(embedder_id TEXT,face_id INTEGER,group_id INTEGER,
            PRIMARY KEY(embedder_id,face_id));

CREATE TABLE face_group_state(embedder_id TEXT PRIMARY KEY,configuration TEXT NOT NULL);

CREATE TABLE face_maintenance(embedder_id TEXT PRIMARY KEY,
            after_photo INTEGER NOT NULL DEFAULT 0, generation INTEGER NOT NULL DEFAULT 1);

CREATE TABLE face_reference_dirty(embedder_id TEXT,person_id INTEGER,PRIMARY KEY(embedder_id,person_id));

CREATE TABLE metadata_photos(
            file_id INTEGER,photo_id INTEGER PRIMARY KEY,relpath TEXT,name TEXT,ext TEXT,
            size INTEGER,meta_state TEXT,dt_original TEXT,model TEXT,lens TEXT,iso INTEGER,camera TEXT);

CREATE TABLE face_sources(photo_id INTEGER PRIMARY KEY,file_id INTEGER NOT NULL);

CREATE TABLE projection_revision(revision INTEGER NOT NULL);

CREATE INDEX ix_faces_photo ON faces(photo_id);

CREATE INDEX ix_faces_person ON faces(person_id);

CREATE INDEX ix_files_photo ON files(photo_id);

CREATE INDEX ix_files_directory_name ON files(dir,name);

CREATE INDEX ix_embeddings_model ON face_embeddings(embedder_id,face_id);

CREATE INDEX ix_training_model ON training_set(embedder_id,split,person_id,face_id);

CREATE INDEX ix_training_face ON training_set(face_id);

CREATE INDEX ix_review_model ON face_review(embedder_id,face_id);

CREATE INDEX ix_faces_review ON faces(person_id,id) WHERE ignored=0 AND retained=0 AND assignment IN ('suggested','auto');

CREATE INDEX ix_faces_unknown ON faces(id,quality) WHERE ignored=0 AND retained=0 AND person_id IS NULL;

CREATE INDEX ix_faces_reference ON faces(person_id,id) WHERE assignment='user' AND ignored=0;

CREATE INDEX ix_faces_match_queue ON faces(photo_id,id) WHERE ignored=0 AND retained=0 AND assignment IS NOT 'user';

CREATE INDEX ix_faces_active_photo ON faces(photo_id,id) WHERE ignored=0;

CREATE INDEX ix_jobs_state ON face_jobs(status,photo_id);

CREATE INDEX ix_group_signature ON face_group_catalog(embedder_id,signature,members,group_id);

CREATE INDEX ix_group_dirty_face ON face_group_dirty(face_id);

CREATE INDEX ix_group_members ON face_group_members(group_id,face_id);

CREATE INDEX ix_group_member_face ON face_group_members(face_id);

CREATE INDEX ix_browse_date ON metadata_photos(dt_original,photo_id);

CREATE INDEX ix_browse_name ON metadata_photos(name COLLATE NOCASE,photo_id);

CREATE INDEX ix_browse_camera ON metadata_photos(camera,photo_id);

CREATE INDEX ix_browse_iso ON metadata_photos(CAST(iso AS REAL),photo_id);

CREATE INDEX ix_browse_year ON metadata_photos(substr(dt_original,1,4),photo_id);

CREATE INDEX ix_group_page ON face_group_catalog(embedder_id,members DESC,group_id) WHERE members>0;

CREATE INDEX ix_faces_user_counts ON faces(person_id) WHERE assignment='user' AND retained=0;

CREATE INDEX ix_faces_ignored ON faces(id) WHERE ignored=1 AND retained=0;

CREATE INDEX ix_faces_retained ON faces(id) WHERE retained=1;

CREATE TRIGGER browse_files_insert AFTER INSERT ON files BEGIN DELETE FROM metadata_photos WHERE photo_id=new.photo_id;INSERT INTO metadata_photos 
SELECT f.id AS file_id,f.photo_id,f.relpath,f.name,f.ext,f.size,f.meta_state,
 m.dt_original,m.model,m.lens,m.iso,NULLIF(TRIM(m.model),'') AS camera
FROM files f LEFT JOIN meta m ON m.file_id=f.id
WHERE f.photo_id=new.photo_id AND f.status='present' AND f.ext!='xmp'
ORDER BY (f.meta_state='done') DESC,(m.dt_original IS NOT NULL) DESC,
 f.ext IN ('nef','nrw','dng') DESC,f.id LIMIT 1
;DELETE FROM face_sources WHERE photo_id=new.photo_id;INSERT INTO face_sources SELECT photo_id,id FROM files WHERE photo_id=new.photo_id
            AND status='present' AND ext IN ('jpg','jpeg','png','tif','tiff','nef','nrw','dng')
            ORDER BY CASE WHEN ext IN ('jpg','jpeg') THEN 0 WHEN ext IN ('png','tif','tiff') THEN 1 ELSE 2 END,id LIMIT 1;UPDATE projection_revision SET revision=revision+1; END;

CREATE TRIGGER browse_files_update AFTER UPDATE OF photo_id,status,relpath,name,ext,size,meta_state ON files BEGIN DELETE FROM metadata_photos WHERE photo_id=old.photo_id;INSERT INTO metadata_photos 
SELECT f.id AS file_id,f.photo_id,f.relpath,f.name,f.ext,f.size,f.meta_state,
 m.dt_original,m.model,m.lens,m.iso,NULLIF(TRIM(m.model),'') AS camera
FROM files f LEFT JOIN meta m ON m.file_id=f.id
WHERE f.photo_id=old.photo_id AND f.status='present' AND f.ext!='xmp'
ORDER BY (f.meta_state='done') DESC,(m.dt_original IS NOT NULL) DESC,
 f.ext IN ('nef','nrw','dng') DESC,f.id LIMIT 1
;DELETE FROM face_sources WHERE photo_id=old.photo_id;INSERT INTO face_sources SELECT photo_id,id FROM files WHERE photo_id=old.photo_id
            AND status='present' AND ext IN ('jpg','jpeg','png','tif','tiff','nef','nrw','dng')
            ORDER BY CASE WHEN ext IN ('jpg','jpeg') THEN 0 WHEN ext IN ('png','tif','tiff') THEN 1 ELSE 2 END,id LIMIT 1;DELETE FROM metadata_photos WHERE photo_id=new.photo_id;INSERT INTO metadata_photos 
SELECT f.id AS file_id,f.photo_id,f.relpath,f.name,f.ext,f.size,f.meta_state,
 m.dt_original,m.model,m.lens,m.iso,NULLIF(TRIM(m.model),'') AS camera
FROM files f LEFT JOIN meta m ON m.file_id=f.id
WHERE f.photo_id=new.photo_id AND f.status='present' AND f.ext!='xmp'
ORDER BY (f.meta_state='done') DESC,(m.dt_original IS NOT NULL) DESC,
 f.ext IN ('nef','nrw','dng') DESC,f.id LIMIT 1
;DELETE FROM face_sources WHERE photo_id=new.photo_id;INSERT INTO face_sources SELECT photo_id,id FROM files WHERE photo_id=new.photo_id
            AND status='present' AND ext IN ('jpg','jpeg','png','tif','tiff','nef','nrw','dng')
            ORDER BY CASE WHEN ext IN ('jpg','jpeg') THEN 0 WHEN ext IN ('png','tif','tiff') THEN 1 ELSE 2 END,id LIMIT 1;UPDATE projection_revision SET revision=revision+1; END;

CREATE TRIGGER browse_files_delete AFTER DELETE ON files BEGIN DELETE FROM metadata_photos WHERE photo_id=old.photo_id;INSERT INTO metadata_photos 
SELECT f.id AS file_id,f.photo_id,f.relpath,f.name,f.ext,f.size,f.meta_state,
 m.dt_original,m.model,m.lens,m.iso,NULLIF(TRIM(m.model),'') AS camera
FROM files f LEFT JOIN meta m ON m.file_id=f.id
WHERE f.photo_id=old.photo_id AND f.status='present' AND f.ext!='xmp'
ORDER BY (f.meta_state='done') DESC,(m.dt_original IS NOT NULL) DESC,
 f.ext IN ('nef','nrw','dng') DESC,f.id LIMIT 1
;DELETE FROM face_sources WHERE photo_id=old.photo_id;INSERT INTO face_sources SELECT photo_id,id FROM files WHERE photo_id=old.photo_id
            AND status='present' AND ext IN ('jpg','jpeg','png','tif','tiff','nef','nrw','dng')
            ORDER BY CASE WHEN ext IN ('jpg','jpeg') THEN 0 WHEN ext IN ('png','tif','tiff') THEN 1 ELSE 2 END,id LIMIT 1;UPDATE projection_revision SET revision=revision+1; END;

CREATE TRIGGER browse_meta_insert AFTER INSERT ON meta BEGIN DELETE FROM metadata_photos WHERE photo_id=(SELECT photo_id FROM files WHERE id=new.file_id);INSERT INTO metadata_photos 
SELECT f.id AS file_id,f.photo_id,f.relpath,f.name,f.ext,f.size,f.meta_state,
 m.dt_original,m.model,m.lens,m.iso,NULLIF(TRIM(m.model),'') AS camera
FROM files f LEFT JOIN meta m ON m.file_id=f.id
WHERE f.photo_id=(SELECT photo_id FROM files WHERE id=new.file_id) AND f.status='present' AND f.ext!='xmp'
ORDER BY (f.meta_state='done') DESC,(m.dt_original IS NOT NULL) DESC,
 f.ext IN ('nef','nrw','dng') DESC,f.id LIMIT 1
;UPDATE projection_revision SET revision=revision+1; END;

CREATE TRIGGER browse_meta_update AFTER UPDATE OF dt_original,model,lens,iso ON meta BEGIN DELETE FROM metadata_photos WHERE photo_id=(SELECT photo_id FROM files WHERE id=old.file_id);INSERT INTO metadata_photos 
SELECT f.id AS file_id,f.photo_id,f.relpath,f.name,f.ext,f.size,f.meta_state,
 m.dt_original,m.model,m.lens,m.iso,NULLIF(TRIM(m.model),'') AS camera
FROM files f LEFT JOIN meta m ON m.file_id=f.id
WHERE f.photo_id=(SELECT photo_id FROM files WHERE id=old.file_id) AND f.status='present' AND f.ext!='xmp'
ORDER BY (f.meta_state='done') DESC,(m.dt_original IS NOT NULL) DESC,
 f.ext IN ('nef','nrw','dng') DESC,f.id LIMIT 1
;DELETE FROM metadata_photos WHERE photo_id=(SELECT photo_id FROM files WHERE id=new.file_id);INSERT INTO metadata_photos 
SELECT f.id AS file_id,f.photo_id,f.relpath,f.name,f.ext,f.size,f.meta_state,
 m.dt_original,m.model,m.lens,m.iso,NULLIF(TRIM(m.model),'') AS camera
FROM files f LEFT JOIN meta m ON m.file_id=f.id
WHERE f.photo_id=(SELECT photo_id FROM files WHERE id=new.file_id) AND f.status='present' AND f.ext!='xmp'
ORDER BY (f.meta_state='done') DESC,(m.dt_original IS NOT NULL) DESC,
 f.ext IN ('nef','nrw','dng') DESC,f.id LIMIT 1
;UPDATE projection_revision SET revision=revision+1; END;

CREATE TRIGGER browse_meta_delete AFTER DELETE ON meta BEGIN DELETE FROM metadata_photos WHERE photo_id=(SELECT photo_id FROM files WHERE id=old.file_id);INSERT INTO metadata_photos 
SELECT f.id AS file_id,f.photo_id,f.relpath,f.name,f.ext,f.size,f.meta_state,
 m.dt_original,m.model,m.lens,m.iso,NULLIF(TRIM(m.model),'') AS camera
FROM files f LEFT JOIN meta m ON m.file_id=f.id
WHERE f.photo_id=(SELECT photo_id FROM files WHERE id=old.file_id) AND f.status='present' AND f.ext!='xmp'
ORDER BY (f.meta_state='done') DESC,(m.dt_original IS NOT NULL) DESC,
 f.ext IN ('nef','nrw','dng') DESC,f.id LIMIT 1
;UPDATE projection_revision SET revision=revision+1; END;

CREATE TRIGGER group_embedding_insert
            AFTER INSERT ON face_embeddings BEGIN
            UPDATE face_group_catalog SET members=max(0,members-1) WHERE group_id IN
                (SELECT group_id FROM face_group_members WHERE face_id=new.face_id AND embedder_id=new.embedder_id);
            DELETE FROM face_group_members WHERE face_id=new.face_id AND embedder_id=new.embedder_id;
            DELETE FROM face_group_dirty WHERE face_id=new.face_id AND embedder_id=new.embedder_id;
            INSERT OR IGNORE INTO face_group_dirty SELECT new.embedder_id,new.face_id FROM faces WHERE id=new.face_id AND person_id IS NULL AND ignored=0 AND retained=0;
            END;

CREATE TRIGGER group_embedding_update
            AFTER UPDATE ON face_embeddings BEGIN
            UPDATE face_group_catalog SET members=max(0,members-1) WHERE group_id IN
                (SELECT group_id FROM face_group_members WHERE face_id=old.face_id AND embedder_id=old.embedder_id);
            DELETE FROM face_group_members WHERE face_id=old.face_id AND embedder_id=old.embedder_id;
            DELETE FROM face_group_dirty WHERE face_id=old.face_id AND embedder_id=old.embedder_id;
            INSERT OR IGNORE INTO face_group_dirty SELECT new.embedder_id,new.face_id FROM faces WHERE id=new.face_id AND person_id IS NULL AND ignored=0 AND retained=0;
            END;

CREATE TRIGGER group_embedding_delete
            AFTER DELETE ON face_embeddings BEGIN
            UPDATE face_group_catalog SET members=max(0,members-1) WHERE group_id IN
                (SELECT group_id FROM face_group_members WHERE face_id=old.face_id AND embedder_id=old.embedder_id);
            DELETE FROM face_group_members WHERE face_id=old.face_id AND embedder_id=old.embedder_id;
            DELETE FROM face_group_dirty WHERE face_id=old.face_id AND embedder_id=old.embedder_id;
            
            END;

CREATE TRIGGER group_face_delete AFTER DELETE ON faces BEGIN
        UPDATE face_group_catalog SET members=max(0,members-1) WHERE group_id IN
            (SELECT group_id FROM face_group_members WHERE face_id=old.id);
        DELETE FROM face_group_members WHERE face_id=old.id;
        DELETE FROM face_group_dirty WHERE face_id=old.id; END;

CREATE TRIGGER user_face_maintenance AFTER UPDATE OF person_id,assignment,ignored ON faces
        WHEN (old.assignment='user' OR new.assignment='user')
        AND (old.person_id IS NOT new.person_id OR old.assignment IS NOT new.assignment OR old.ignored IS NOT new.ignored)
        BEGIN
        INSERT OR IGNORE INTO face_reference_dirty
            SELECT embedder_id,old.person_id FROM face_embeddings WHERE face_id=new.id AND old.person_id IS NOT NULL;
        INSERT OR IGNORE INTO face_reference_dirty
            SELECT embedder_id,new.person_id FROM face_embeddings WHERE face_id=new.id AND new.person_id IS NOT NULL;
        INSERT INTO face_maintenance SELECT embedder_id,0,1 FROM face_embeddings WHERE face_id=new.id
            ON CONFLICT(embedder_id) DO UPDATE SET after_photo=0,generation=generation+1;
        END;

CREATE TRIGGER group_face_state
        AFTER UPDATE OF person_id,ignored,retained,quality ON faces
        WHEN old.person_id IS NOT new.person_id OR old.ignored IS NOT new.ignored
            OR old.retained IS NOT new.retained OR old.quality IS NOT new.quality
        BEGIN
        UPDATE face_group_catalog SET members=max(0,members-1) WHERE group_id IN
            (SELECT group_id FROM face_group_members WHERE face_id=new.id);
        DELETE FROM face_group_members WHERE face_id=new.id;
        DELETE FROM face_group_dirty WHERE face_id=new.id;
        INSERT OR IGNORE INTO face_group_dirty SELECT embedder_id,new.id FROM face_embeddings
            WHERE face_id=new.id AND new.person_id IS NULL AND new.ignored=0 AND new.retained=0;
        END;

PRAGMA user_version=6;