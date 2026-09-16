SELECT CAST(selected.key AS INTEGER),card.downloaded,card.download_requested
FROM json_each(?1) selected JOIN book_cards card ON card.content_hash=selected.value
