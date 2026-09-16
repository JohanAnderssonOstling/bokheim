INSERT INTO rejected_asset_upload(kind,hash,reason,rejected_at) VALUES(?1,?2,?3,?4)
         ON CONFLICT(kind,hash) DO UPDATE SET reason=excluded.reason,rejected_at=excluded.rejected_at