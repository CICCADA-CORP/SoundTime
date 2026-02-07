<script lang="ts">
  export let data: number[] = [];
  export let progress = 0;
  export let height = 60;

  let containerWidth = 0;

  $: barWidth = data.length > 0 ? Math.max(1, containerWidth / data.length - 1) : 2;
  $: progressPercent = progress * 100;
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
