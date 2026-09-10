`schema_v9.sqlite` is a real Ferry store database frozen at schema migration 9, with a
representative peer / outbound item / audit row. `migrate::tests` copies it to a temp file
and runs the full migration chain against it, so a forward migration is exercised on a
populated on-disk database, not just a fresh one.

Regenerate after adding or changing migrations at or below version 9:

    cargo test -p ferry-store -- --ignored regenerate_schema_v9_fixture
