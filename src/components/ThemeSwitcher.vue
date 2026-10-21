<script setup lang="ts">
import { appearance,themes,setTheme,setMotion } from '../appearance'
import Icon from './Icon.vue'
</script>
<template><div class="appearance-tools"><div class="theme-switcher" role="group" aria-label="切换主题 / Theme"><button v-for="theme in themes" :key="theme.id" type="button" :class="['theme-dot',theme.id,{active:appearance.theme===theme.id}]" :aria-label="`${theme.name} / ${theme.english}`" :title="`${theme.name} / ${theme.english}`" :aria-pressed="appearance.theme===theme.id" @click="setTheme(theme.id)"><span :style="{background:theme.color}"/></button></div><button type="button" class="motion-switch" :aria-pressed="appearance.motion" :aria-label="appearance.motion?'关闭动效 / Reduce motion':'开启动效 / Enable motion'" :title="appearance.motion?'关闭动效 / Reduce motion':'开启动效 / Enable motion'" @click="setMotion(!appearance.motion)"><Icon :name="appearance.motion?'play':'stop'" :size="14"/></button></div></template>
