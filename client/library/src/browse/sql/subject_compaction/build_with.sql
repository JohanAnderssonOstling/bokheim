WITH RECURSIVE direct(concept_id,hash) AS (
                SELECT assignment.concept_id,book.content_hash FROM json_each(?1) selected
                JOIN book ON book.content_hash=selected.value
                JOIN book_unified_concept assignment ON assignment.book_row_id=book.row_id
            ), used(path,parent_path,concept_id,label) AS (
                SELECT route.path,route.parent_path,route.concept_id,route.label
                FROM direct JOIN library_subject route ON route.concept_id=direct.concept_id
                UNION
                SELECT parent.path,parent.parent_path,parent.concept_id,parent.label
                FROM used child JOIN library_subject parent ON parent.path=child.parent_path
                WHERE child.path!=child.parent_path
            )
            SELECT used.path,used.parent_path,used.label,direct.hash
            FROM used
            LEFT JOIN direct ON direct.concept_id=used.concept_id
