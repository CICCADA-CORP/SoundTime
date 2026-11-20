import assert from "node:assert/strict";
import test from "node:test";
import {setImmediate as nextTurn} from "node:timers/promises";
import {pageDocuments} from "./browser-document.mjs";

const frameId = "main";
function commit(page, loaderId) {
  page.observe({method:"Page.frameNavigated", params:{frame:{id:frameId, loaderId}}});
}
function create(page, uniqueId, frame = frameId) {
  page.observe({method:"Runtime.executionContextCreated",
    params:{context:{id:1, uniqueId, auxData:{frameId:frame, isDefault:true}}}});
}
function destroy(page, uniqueId) {
  page.observe({method:"Runtime.executionContextDestroyed",
    params:{executionContextId:1, executionContextUniqueId:uniqueId}});
}
const protocolError = (code, message) => Object.assign(new Error(message), {code});

test("acknowledged navigation waits for its loader and new main-frame context", async () => {
  const page = pageDocuments(async method => {
    assert.equal(method, "Page.navigate");
    return {frameId, loaderId:"two"};
  });
  commit(page, "one");
  create(page, "old");
  let ready = false;
  const navigation = page.navigate("about:blank").then(() => { ready = true; });
  await nextTurn();
  commit(page, "two");
  create(page, "iframe", "child");
  await nextTurn();
  assert.equal(ready, false);
  create(page, "new");
  destroy(page, "old"); // Chromium reused numeric ID 1 in the new renderer.
  await navigation;
  assert.equal(ready, true);
});

test("only pure observations retry after Chrome has lost the previous document", async () => {
  for (const error of [
    protocolError(-32000, "Cannot find context with specified id"),
    protocolError(-32602, "uniqueContextId not found"),
  ]) {
    const calls = [];
    const page = pageDocuments(async (method, {uniqueContextId, arguments: args}) => {
      assert.equal(method, "Runtime.callFunctionOn");
      assert.deepEqual(args, [{value:"quoted fixture"}]);
      calls.push(uniqueContextId);
      if (uniqueContextId === "old") {
        destroy(page, "old");
        create(page, "new");
        throw error;
      }
      return "observed";
    });
    commit(page, "one");
    create(page, "old");
    assert.equal(await page.call({arguments:[{value:"quoted fixture"}]}, true), "observed");
    assert.deepEqual(calls, ["old", "new"]);
  }
});

test("a committed initial document can keep its global once its own DOM is ready", async () => {
  const page = pageDocuments(async () => ({frameId, loaderId:"blank"}));
  commit(page, "initial");
  create(page, "global");
  const navigation = page.navigate("about:blank");
  commit(page, "blank");
  page.observe({method:"Page.lifecycleEvent", params:{frameId, loaderId:"blank", name:"DOMContentLoaded"}});
  await navigation;
});

test("mutations are never replayed and unrelated read failures propagate", async () => {
  for (const [read, error] of [
    [false, protocolError(-32000, "Execution context was destroyed.")],
    [true, protocolError(-32602, "invalid uniqueContextId")],
  ]) {
    let calls = 0;
    const page = pageDocuments(async () => {
      calls++;
      create(page, "new");
      throw error;
    });
    commit(page, "one");
    create(page, "old");
    await assert.rejects(page.call({}, read), error);
    assert.equal(calls, 1);
  }
});
