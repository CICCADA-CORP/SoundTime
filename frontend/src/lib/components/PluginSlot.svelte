<script lang="ts">
  /**
   * PluginSlot — renders plugin UI via sandboxed iframes.
   *
   * Each enabled plugin with UI for this slot gets its own `<iframe>`
   * with `sandbox="allow-scripts"` (no `allow-same-origin`).
   * Communication happens via the `postMessage` API.
   */
  import type { Plugin } from "$lib/types";
  import { pluginApi } from "$lib/api";

  interface Props {
    /** The slot name (e.g. "track-detail-sidebar", "player-extra-controls"). */
    slot: string;
    /** Optional context data passed to plugin iframes via postMessage. */
    context?: Record<string, unknown>;
    /** CSS class for the container. */
    class?: string;
  }

  let { slot, context = {}, class: className = "" }: Props = $props();

  let plugins = $state<Plugin[]>([]);
  let loading = $state(true);
  let iframeRefs = $state<Map<string, HTMLIFrameElement>>(new Map());

  /** Load plugins that have UI enabled. */
  async function loadSlotPlugins() {
    loading = true;
    try {
      const response = await pluginApi.list();
      // Filter to enabled plugins with permissions.
      // Note: Slot-specific filtering is not possible from the API alone since
      // the plugin DB model doesn't expose UI slot info. Iframes that fail to
      // load (404) are hidden via the error handler.
      plugins = response.plugins.filter(
        (p) => p.status === "enabled" && p.permissions
      );
    } catch {
      plugins = [];
    }
    loading = false;
  }

  /** Send context data to a plugin iframe via postMessage. */
  function sendContext(iframe: HTMLIFrameElement, plugin: Plugin) {
    if (!iframe.contentWindow) return;
    iframe.contentWindow.postMessage(
      {
        type: "soundtime:context",
        slot,
        pluginId: plugin.id,
        data: context,
      },
      "*"
    );
  }

  /** Handle messages from plugin iframes. */
  function handleMessage(event: MessageEvent) {
    if (!event.data || typeof event.data !== "object") return;
    if (event.data.type !== "soundtime:plugin-action") return;

    // SECURITY: Validate that the message source is one of our plugin iframes
    const isFromPluginIframe = Array.from(iframeRefs.values()).some(
      (iframe) => iframe.contentWindow === event.source
    );
    if (!isFromPluginIframe) return;

    const { pluginId, action, payload } = event.data;
    window.dispatchEvent(
      new CustomEvent("soundtime:plugin-action", {
        detail: { pluginId, action, payload, slot },
      })
    );
  }

  /** Handle iframe load errors — hide the frame if the plugin has no UI for this slot. */
  function handleIframeError(pluginId: string) {
    plugins = plugins.filter((p) => p.id !== pluginId);
    iframeRefs.delete(pluginId);
  }

  /** Register an iframe ref and send initial context. */
  function registerIframe(node: HTMLIFrameElement, plugin: Plugin) {
    iframeRefs.set(plugin.id, node);
    node.addEventListener("load", () => {
      // Check if the iframe loaded successfully (won't have content on 404)
      sendContext(node, plugin);
    });
    node.addEventListener("error", () => handleIframeError(plugin.id));
  }

  $effect(() => {
    loadSlotPlugins();
  });

  // Listen for postMessage from iframes
  $effect(() => {
    window.addEventListener("message", handleMessage);
    return () => window.removeEventListener("message", handleMessage);
  });

  // Send updated context to all iframes when context changes
  $effect(() => {
    if (context) {
      for (const [pluginId, iframe] of iframeRefs) {
        const plugin = plugins.find((p) => p.id === pluginId);
        if (plugin) sendContext(iframe, plugin);
      }
    }
  });
</script>

{#if plugins.length > 0}
  <div class="plugin-slot plugin-slot--{slot} {className}">
    {#each plugins as plugin (plugin.id)}
      <div class="plugin-slot__frame">
        <iframe
          use:registerIframe={plugin}
          src="/api/admin/plugins/{plugin.id}/ui/{slot}"
          sandbox="allow-scripts"
          title="{plugin.name} ({slot})"
          loading="lazy"
          class="w-full border-0"
          style="min-height: 64px;"
        ></iframe>
      </div>
    {/each}
  </div>
{/if}
