import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";

class Element {
  constructor(tag) {
    this.tag = tag;
    this.children = [];
    this.events = {};
  }
  append(...children) { this.children.push(...children); }
  addEventListener(name, listener) { this.events[name] = listener; }
  remove() { this.removed = true; }
}

function fixture(collapsed = false) {
  const root = new Element("div");
  const disclosure = collapsed ? new Element("details") : null;
  const listeners = new Map();
  const window = {
    addEventListener: (name, listener) => listeners.set(name, listener),
    removeEventListener: name => listeners.delete(name),
  };
  const document = {
    getElementById: id => id.endsWith("-details") ? disclosure : root,
    createElement: tag => {
      const node = new Element(tag);
      if (tag === "iframe") node.contentWindow = {};
      return node;
    },
  };
  const context = vm.createContext({ window, document });
  vm.runInContext(readFileSync(new URL("../../assets/coverage-viewer.js", import.meta.url), "utf8"), context);
  return { root, disclosure, listeners, render: context.habeasRenderCoverage };
}

function contents(node) {
  return [node.textContent ?? "", ...node.children.map(contents)].join("\n");
}

// {test}`requirements.test-coverage.index.details`
test("process reports preserve counters, source text, child identity and lazy file rendering", () => {
  const { root, listeners, render } = fixture();
  render({
    target: "report", source: "snapshot.html", run: "run-1",
    processes: [{
      kind: "Test", pid: 1, profiles: ["1-a.profraw"],
      lines: { "code.rs": { 1: "18446744073709551615", 2: "0" } },
    }, {
      kind: "Child", pid: 2, profiles: ["2-a.profraw"],
      lines: { "other.rs": { 1: "0" } },
    }],
  });
  const frame = root.children.find(node => node.tag === "iframe");
  assert.equal(frame.src, "snapshot.html");
  const receive = listeners.get("message");
  const data = { kind: "habeas-coverage-source", run: "run-1",
    sources: { "code.rs": "<script>captured text</script>\nnever reached", "other.rs": "child source" } };
  receive({ source: {}, data }); // An unrelated window cannot supply source.
  assert.equal(frame.removed, undefined);
  receive({ source: frame.contentWindow, data: { ...data, run: "other-run" } });
  assert.equal(frame.removed, undefined);
  receive({ source: frame.contentWindow, data });
  assert.equal(frame.removed, true);
  assert.equal(listeners.has("message"), false);
  assert.match(contents(root), /18446744073709551615 executed/);
  assert.match(contents(root), /0 unexecuted/);
  assert.match(contents(root), /<script>captured text<\/script>/);
  assert.match(contents(root), /Child process 2/);
  const closed = root.children.filter(node => node.tag === "details")[1];
  assert.doesNotMatch(contents(closed), /child source/);
  closed.open = true;
  closed.events.toggle();
  assert.match(contents(closed), /child source/);
  const size = closed.children.length;
  closed.events.toggle();
  assert.equal(closed.children.length, size);
});

// {test}`requirements.test-coverage.index.file_map.test`
test("line-level source is loaded only when requested, once per report", () => {
  const { root, disclosure, listeners, render } = fixture(true);
  render({ target: "report", source: "snapshot.html", run: "run-1", processes: [] });
  assert.equal(root.children.length, 0);
  assert.equal(listeners.has("message"), false);
  disclosure.open = true;
  disclosure.events.toggle();
  assert.equal(root.children.filter(node => node.tag === "iframe").length, 1);
  assert.equal(listeners.has("message"), true);
  const size = root.children.length;
  disclosure.open = false;
  disclosure.events.toggle();
  disclosure.open = true;
  disclosure.events.toggle();
  assert.equal(root.children.length, size);
});
