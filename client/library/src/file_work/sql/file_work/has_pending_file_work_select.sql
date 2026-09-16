SELECT EXISTS(SELECT 1 FROM local_directory_work)
        OR EXISTS(SELECT 1 FROM local_file_work)
        OR EXISTS(SELECT 1 FROM local_book_work)