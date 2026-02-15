<script lang="ts">
  import StatCard from './StatCard.svelte';
  let { value = '42', label = 'Test Label' }: { value?: string | number; label?: string } = $props();
</script>
<StatCard {value} {label}>
  {#snippet icon()}
    <span data-testid="stat-icon">📊</span>
  {/snippet}
</StatCard>
