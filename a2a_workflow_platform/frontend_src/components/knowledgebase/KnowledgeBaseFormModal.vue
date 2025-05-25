<template>
  <div v-if="show" class="fixed inset-0 bg-gray-600 bg-opacity-50 overflow-y-auto h-full w-full flex justify-center items-center z-50" @click.self="closeModal">
    <div class="relative mx-auto p-5 border w-full max-w-md shadow-lg rounded-md bg-white">
      <div class="mt-3 text-center">
        <h3 class="text-lg leading-6 font-medium text-gray-900 mb-4">{{ formTitle }}</h3>
        <el-form :model="formData" :rules="rules" ref="formRef" label-width="60px" status-icon>
          <el-form-item label="名称" prop="name">
            <el-input v-model="formData.name" maxlength="50" show-word-limit placeholder="请输入知识库名称" />
          </el-form-item>
          <el-form-item label="描述" prop="description">
            <el-input
              v-model="formData.description"
              type="textarea"
              :rows="3"
              maxlength="200"
              show-word-limit
              placeholder="可选，最多200字"
            />
          </el-form-item>
          <el-form-item label="公开" prop="is_public">
            <el-switch v-model="formData.is_public" />
            <span class="text-xs text-gray-500 ml-2">{{ formData.is_public ? '所有人可见' : '仅自己可见' }}</span>
          </el-form-item>
          <div class="items-center gap-2 mt-3 sm:flex">
            <el-button type="default" @click="closeModal" class="w-full mt-2 flex-1" :disabled="store.loading.saveKb">取消</el-button>
            <el-button type="primary" @click="submitForm" class="w-full mt-2 flex-1" :loading="store.loading.saveKb">{{ submitButtonText }}</el-button>
          </div>
        </el-form>
      </div>
    </div>
  </div>
</template>

<script setup>
import { ref, watch, computed } from 'vue';
import { ElForm, ElFormItem, ElInput, ElButton, ElMessage } from 'element-plus';
import { useKnowledgeBaseStore } from '@/store/knowledgebaseStore';

const props = defineProps({
  show: Boolean,
  knowledgeBase: { // 用于编辑，如果是创建新知识库则为 null
    type: Object,
    default: null,
  },
});

const emit = defineEmits(['close']);

const store = useKnowledgeBaseStore();

const initialFormData = () => ({
  id: null,
  name: '',
  description: '',
  is_public: false, // 默认新建为私有
});

const formData = ref(initialFormData());
const formRef = ref(null);

watch(() => props.knowledgeBase, (newVal) => {
  if (newVal && newVal.id) {
    formData.value = { ...newVal };
  } else {
    formData.value = initialFormData();
  }
}, { immediate: true });

watch(() => props.show, (newVal) => {
  if (newVal && props.knowledgeBase && props.knowledgeBase.id) {
    formData.value = { ...props.knowledgeBase };
  } else if (newVal) {
    formData.value = initialFormData();
  }
});

const rules = {
  name: [
    { required: true, message: '名称不能为空', trigger: 'blur' },
    { min: 2, max: 50, message: '名称长度需为2-50个字符', trigger: 'blur' }
  ],
  description: [
    { max: 200, message: '描述最多200个字符', trigger: 'blur' }
  ]
};

const formTitle = computed(() => {
  return formData.value.id ? '编辑知识库' : '创建新知识库';
});

const submitButtonText = computed(() => {
  if (store.loading.saveKb) {
    return formData.value.id ? '保存中...' : '创建中...';
  }
  return formData.value.id ? '保存更改' : '创建';
});

const closeModal = () => {
  emit('close');
};

const submitForm = async () => {
  if (!formRef.value) return;
  await formRef.value.validate(async (valid) => {
    if (valid) {
      const dataToSave = {
        name: formData.value.name,
        description: formData.value.description,
        is_public: formData.value.is_public, // 添加 is_public 到保存数据中
      };
      try {
        if (formData.value.id) {
          await store.updateKnowledgeBase(formData.value.id, dataToSave);
        } else {
          await store.createKnowledgeBase(dataToSave);
        }
        closeModal();
      } catch (error) {
        // Errors are already handled by the store (ElMessage)
        // console.error("Failed to save knowledge base:", error); 
        // No need to show another message here unless it's specific
      }
    } else {
      ElMessage.error('请检查表单输入');
      return false;
    }
  });
};

</script>

<style scoped>
/* 可以在这里添加模态框的特定样式 */
</style> 