SELECT
                    COUNT(*),
                    COUNT(*) FILTER (WHERE state_kind='book_lifecycle'),
                    COUNT(*) FILTER (WHERE state_kind='placement'),
                    COUNT(*) FILTER (WHERE state_kind='metadata')
                 FROM sync_outbox