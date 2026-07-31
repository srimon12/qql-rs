import test from "node:test";
import assert from "node:assert/strict";
import { rawFenceFailures } from "./verify-qql-docs.mjs";

const VIRTUAL_FILE = "virtual.mdoc";

test("flags lowercase qql/sql fences outside qqlExample", () => {
  const source = [
    "## Example",
    "```sql",
    "SELECT * FROM docs;",
    "```",
    "",
    "```qql",
    "QUERY 'text' FROM docs",
    "```",
  ].join("\n");
  const failures = rawFenceFailures(source, VIRTUAL_FILE);
  assert.equal(failures.length, 2);
  assert.deepEqual(
    failures.map((failure) => failure.line),
    [3, 7],
  );
  assert.ok(
    failures.every((failure) =>
      /bare .* fence outside qqlExample/.test(failure.message),
    ),
  );
});

test("flags uppercase QQL/SQL fences outside qqlExample", () => {
  const source = [
    "```SQL",
    "SELECT * FROM docs;",
    "```",
    "",
    "```QQL",
    "QUERY 'text' FROM docs",
    "```",
  ].join("\n");
  const failures = rawFenceFailures(source, VIRTUAL_FILE);
  assert.equal(failures.length, 2);
  assert.deepEqual(
    failures.map((failure) => failure.message),
    [
      "bare ```SQL fence outside qqlExample; wrap runnable QQL in {% qqlExample %} or use a text/ebnf fence",
      "bare ```QQL fence outside qqlExample; wrap runnable QQL in {% qqlExample %} or use a text/ebnf fence",
    ],
  );
});

test("flags mixed-case qql/sql fences outside qqlExample", () => {
  for (const language of ["Qql", "sQL", "SqL"]) {
    const source = [
      `\`\`\`${language}`,
      "QUERY 'text' FROM docs",
      "```",
    ].join("\n");
    const failures = rawFenceFailures(source, VIRTUAL_FILE);
    assert.equal(failures.length, 1, `${language} fence should be flagged`);
    assert.equal(failures[0].line, 2);
  }
});

test("masks qql/sql fences inside qqlExample regardless of case", () => {
  for (const language of ["sql", "SQL", "qql", "QQL"]) {
    const source = [
      "{% qqlExample %}",
      `\`\`\`${language}`,
      "QUERY 'text' FROM docs",
      "```",
      "{% /qqlExample %}",
    ].join("\n");
    assert.deepEqual(
      rawFenceFailures(source, VIRTUAL_FILE),
      [],
      `${language} fence inside qqlExample should be masked`,
    );
  }
});

test("ignores non-qql/sql fences and bare fences", () => {
  const source = [
    "```ebnf",
    "query = QUERY string FROM collection;",
    "```",
    "",
    "```",
    "plain text",
    "```",
    "",
    "```rust",
    "fn main() {}",
    "```",
  ].join("\n");
  assert.deepEqual(rawFenceFailures(source, VIRTUAL_FILE), []);
});

test("preserves line numbers across qqlExample masking", () => {
  const source = [
    "line one",
    "{% qqlExample %}",
    "```sql",
    "SELECT * FROM docs;",
    "```",
    "{% /qqlExample %}",
    "after",
    "```QQL",
    "QUERY 'text' FROM docs",
    "```",
  ].join("\n");
  const failures = rawFenceFailures(source, VIRTUAL_FILE);
  assert.equal(failures.length, 1);
  assert.equal(failures[0].line, 9);
});
