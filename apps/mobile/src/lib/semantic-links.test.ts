import assert from "node:assert/strict";
import test from "node:test";

import { createBookLink, reviseBookLink } from "./semantic-links.ts";

test("semantic link helpers keep creation and revision timestamps valid", () => {
  const created = createBookLink("1", "2", "  contrasts with  ", "  Read together  ", 100);
  assert.match(created.id, /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
  assert.equal(created.relation, "contrasts with");
  assert.equal(created.note, "Read together");
  assert.equal(created.created_at, 100);
  assert.equal(created.modified_at, 100);

  const revised = reviseBookLink(created, " related to ", "", 50);
  assert.equal(revised.id, created.id);
  assert.equal(revised.relation, "related to");
  assert.equal(revised.modified_at, created.created_at);
});
