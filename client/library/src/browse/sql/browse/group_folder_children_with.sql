WITH RECURSIVE candidates(id,parent_id,name) AS (
            SELECT d.id,d.parent_id,d.name FROM json_each(?1) selected
            JOIN dir parent ON parent.id=selected.value
            JOIN dir d ON d.parent_id=parent.id AND d.id!=parent.id
            WHERE parent.deleted_at IS NULL AND d.deleted_at IS NULL AND d.name NOT LIKE '.%'
              AND NOT EXISTS(SELECT 1 FROM book_dir p JOIN book b ON b.row_id=p.book_row_id
                  WHERE p.dir_id=parent.id AND p.deleted_at IS NULL AND b.deleted_at IS NULL AND b.hidden_at IS NULL)
        ), tree(folder_id,id) AS (
            SELECT id,id FROM candidates
            UNION ALL
            SELECT t.folder_id,d.id FROM tree t JOIN dir d ON d.parent_id=t.id AND d.id!=t.id
            WHERE d.deleted_at IS NULL AND d.name NOT LIKE '.%'
        ), matched AS (
            SELECT b.row_id FROM book b WHERE b.deleted_at IS NULL AND b.hidden_at IS NULL
            AND (?2=0 OR ((?2 & 1)!=0 AND lower(b.format) NOT IN ('pdf','m4b'))
                OR ((?2 & 2)!=0 AND lower(b.format)='pdf') OR ((?2 & 4)!=0 AND lower(b.format)='m4b'))
            AND (?3='' OR EXISTS(SELECT 1 FROM book_language l WHERE l.book_row_id=b.row_id
                AND (instr(?3,',' || lower(l.language_tag) || ',')>0
                    OR (instr(l.language_tag,'-')>0 AND instr(?3,',' || substr(lower(l.language_tag),1,instr(l.language_tag,'-')-1) || ',')>0))))
        ) SELECT c.parent_id,c.id,c.name,count(DISTINCT b.row_id) AS total,
            count(DISTINCT CASE WHEN p.is_downloaded THEN b.row_id END) AS downloaded
        FROM candidates c JOIN tree t ON t.folder_id=c.id
        LEFT JOIN book_dir p ON p.dir_id=t.id AND p.deleted_at IS NULL
        LEFT JOIN matched b ON b.row_id=p.book_row_id
        GROUP BY c.parent_id,c.id,c.name
        ORDER BY c.parent_id,CASE WHEN ?4=1 THEN CASE WHEN ?5 THEN downloaded ELSE total END END DESC,
            normalize_search_text(c.name),c.name,c.id