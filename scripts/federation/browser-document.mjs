import { EventEmitter, once } from "node:events";

// Page.navigate can acknowledge a navigation before Chrome commits its document.
// Numeric execution context IDs can also be recycled when a renderer changes.
export function pageDocuments(send) {
  const changes = new EventEmitter();
  const contexts = new Map();
  let mainFrame;
  let readyLoader;

  function observe({method, params}) {
    if (method === "Page.frameNavigated" && !params.frame.parentId) {
      mainFrame = params.frame;
    } else if (method === "Page.lifecycleEvent" && params.frameId === mainFrame?.id &&
        params.name === "DOMContentLoaded") {
      readyLoader = params.loaderId;
    } else if (method === "Page.frameDetached") {
      contexts.delete(params.frameId);
    } else if (method === "Runtime.executionContextCreated" && params.context.auxData?.isDefault) {
      contexts.set(params.context.auxData.frameId, params.context.uniqueId);
    } else if (method === "Runtime.executionContextDestroyed") {
      for (const [frame, context] of contexts) {
        if (context === params.executionContextUniqueId) contexts.delete(frame);
      }
    } else if (method === "Runtime.executionContextsCleared") {
      contexts.clear();
    } else {
      return;
    }
    changes.emit("change");
  }

  const currentContext = () => contexts.get(mainFrame?.id);
  async function waitFor(predicate, signal) {
    while (!predicate()) await once(changes, "change", {signal});
  }

  async function navigate(url) {
    const previous = currentContext();
    const {frameId, loaderId, errorText} = await send("Page.navigate", {url});
    if (errorText) throw new Error("Chrome navigation failed: " + errorText);
    if (loaderId) {
      // The initial empty document may retain its global when navigated to about:blank.
      await waitFor(() => mainFrame?.id === frameId && mainFrame.loaderId === loaderId &&
        currentContext() && (currentContext() !== previous || readyLoader === loaderId), AbortSignal.timeout(10000));
    }
  }

  async function call(params, retryRead) {
    const signal = AbortSignal.timeout(10000);
    let rejectedContext;
    for (;;) {
      await waitFor(() => currentContext() && currentContext() !== rejectedContext, signal);
      const uniqueContextId = currentContext();
      try {
        return await send("Runtime.callFunctionOn", {...params, uniqueContextId});
      } catch (error) {
        // Mutations can commit before a navigation destroys their return context.
        // Only pure observations may be repeated, on a different main document.
        const missing = error.code === -32602 && error.message === "uniqueContextId not found";
        const destroyed = error.code === -32000 && (error.message === "Cannot find context with specified id" ||
          error.message.startsWith("Execution context was destroyed"));
        if (!retryRead || !(missing || destroyed)) throw error;
        rejectedContext = uniqueContextId;
      }
    }
  }

  async function enable() {
    await send("Page.enable");
    await send("Page.setLifecycleEventsEnabled", {enabled:true});
    mainFrame = (await send("Page.getFrameTree")).frameTree.frame;
    await send("Runtime.enable");
  }

  return {observe, enable, navigate, call};
}
