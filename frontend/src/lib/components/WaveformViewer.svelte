<script lang="ts">
  let { data = [], progress = 0, height = 60 }: {
    data?: number[];
    progress?: number;
    height?: number;
  } = $props();

  let containerWidth = $state(0);

  let barWidth = $derived(data.length > 0 ? Math.max(1, containerWidth / data.length - 1) : 2);
  let progressPercent = $derived(progress * 100);
</script>

<div class="w-full relative" style="height: {height}px" bind:clientWidth={containerWidth}>
  <svg width="100%" height="100%" preserveAspectRatio="none">
    {#each data as peak, i}
      {@const x = (i / data.length) * 100}
      {@const barH = Math.max(2, peak * height)}
      {@const isPast = x < progressPercent}
      <rect
        x="{x}%"
        y={height - barH}
        width="{100 / data.length * 0.8}%"
        height={barH}
        rx="1"
        fill={isPast ? "hsl(142, 71%, 45%)" : "hsl(0, 0%, 25%)"}
      />
    {/each}
  </svg>
</div>
