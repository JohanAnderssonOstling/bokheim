SELECT max(rejected.reason) FROM rejected_asset_upload rejected
                                        JOIN local_book_upload upload ON upload.content_hash=rejected.hash
                                        WHERE rejected.kind='book' AND rejected.hash=?1 AND rejected.rejected_at>unixepoch()-300