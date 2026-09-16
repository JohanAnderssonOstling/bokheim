SELECT sync_outbox.mutation_id, version.mutation_id
                 FROM sync_outbox JOIN sync_state_version version
                   ON version.state_kind=sync_outbox.state_kind
                  AND version.state_key=sync_outbox.state_key
                  AND version.state_subkey=''
                 WHERE sync_outbox.state_kind='book_lifecycle'