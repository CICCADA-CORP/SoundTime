<script lang="ts">
  import SectionCarousel from './SectionCarousel.svelte';
  let { title = 'Test', href }: { title?: string; href?: string } = $props();
</script>
<SectionCarousel {title} {href}>
  <div data-testid="carousel-child">Child content</div>
</SectionCarousel>
