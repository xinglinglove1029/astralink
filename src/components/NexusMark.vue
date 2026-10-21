<script setup lang="ts">
import markUrl from '../assets/astralink-mark.svg?no-inline'

defineProps<{ size?: number }>()
</script>

<template>
  <span
    class="nexus-mark"
    aria-hidden="true"
    :style="{ width: `${size || 36}px`, height: `${size || 36}px`, maskImage: `url(${markUrl})` }"
  />
</template>

<style scoped>
.nexus-mark {
  display: inline-block;
  flex-shrink: 0;
  vertical-align: middle;
  background: currentColor;
  mask-position: center;
  mask-repeat: no-repeat;
  mask-size: contain;
}
</style>
