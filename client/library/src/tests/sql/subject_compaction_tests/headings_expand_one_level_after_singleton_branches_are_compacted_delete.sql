DELETE FROM book_unified_concept;
         INSERT INTO curated.unified_concept_route VALUES
            ('Domain',929999,'','Domain'),
            ('Domain / Humanities',930000,'Domain','Humanities'),
            ('Domain / Humanities / Wrapper',930001,'Domain / Humanities','Wrapper'),
            ('Domain / Humanities / Wrapper / History',930002,'Domain / Humanities / Wrapper','History'),
            ('Domain / Humanities / Wrapper / History / Ancient',930003,'Domain / Humanities / Wrapper / History','Ancient'),
            ('Domain / Humanities / Wrapper / History / Medieval',930004,'Domain / Humanities / Wrapper / History','Medieval'),
            ('Domain / Humanities / Philosophy',930005,'Domain / Humanities','Philosophy'),
            ('Domain / Science',930006,'Domain','Science');
         INSERT INTO book_unified_concept
            SELECT row_id,concept_id,1 FROM book CROSS JOIN curated.unified_concept_route
            WHERE title IN ('Alpha book','Beta book') AND concept_id IN (930003,930004,930005,930006);