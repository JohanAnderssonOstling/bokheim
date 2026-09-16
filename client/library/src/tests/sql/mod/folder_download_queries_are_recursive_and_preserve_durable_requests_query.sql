INSERT INTO dir(id, parent_id, name) VALUES
                 ('7368656c-6600-0000-0000-000007e1ec43', '00000000-0000-0000-0000-000000000000', 'Shelf'),
                 ('6368696c-6400-0000-0000-0000e5a0205d', '7368656c-6600-0000-0000-000007e1ec43', 'Child'),
                 ('6f746865-7200-0000-0000-0000c87d8df5', '00000000-0000-0000-0000-000000000000', 'Other');
             INSERT INTO book(content_hash, added_at, format) VALUES (printf('%064x',1), 1, 'epub'), (printf('%064x',2), 1, 'epub'), (printf('%064x',3), 1, 'epub');