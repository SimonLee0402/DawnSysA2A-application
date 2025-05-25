<template>
  <div class="task-create">
    <h1>创建任务</h1>
    <el-alert
      title="页面开发中"
      type="info"
      description="此页面正在开发中，敬请期待。"
      show-icon
      :closable="false"
    />
  </div>
</template>

<script>
export default {
  name: 'TaskCreate'
}
</script>

<style scoped>
.task-create {
  padding: 20px;
}
</style> 