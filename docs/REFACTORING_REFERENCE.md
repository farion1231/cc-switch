# 重构快速参考指南

> 常见模式和代码示例的速查表

---

## 📑 目录

1. [React Query 使用](#react-query-使用)
2. [react-hook-form 使用](#react-hook-form-使用)
3. [shadcn/ui 组件使用](#shadcnui-组件使用)
4. [代码迁移示例](#代码迁移示例)

---

## React Query 使用

### 基础查询

```typescript
// 定义查询 Hook
export const useProvidersQuery = (appId: AppId) => {
  return useQuery({
    queryKey: ['providers', appId],
    queryFn: async () => {
      const data = await providersApi.getAll(appId)
      return data
    },
  })
}

// 在组件中使用
function MyComponent() {
  const { data, isLoading, error } = useProvidersQuery('claude')

  if (isLoading) return <div>Loading...</div>
  if (error) return <div>Error: {error.message}</div>

  return <div>{/* 使用 data */}</div>
}
```

### Mutation (变更操作)

```typescript
// 定义 Mutation Hook
export const useAddProviderMutation = (appId: AppId) => {
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: async (provider: Provider) => {
      return await providersApi.add(provider, appId)
    },
    onSuccess: () => {
      // 重新获取数据
      queryClient.invalidateQueries({ queryKey: ['providers', appId] })
      toast.success('添加成功')
    },
    onError: (error: Error) => {
      toast.error(`添加失败: ${error.message}`)
    },
  })
}

// 在组件中使用
function AddProviderDialog() {
const mutation = useAddProviderMutation('claude')

  const handleSubmit = (data: Provider) => {
    mutation.mutate(data)
  }

  return (
    <button
      onClick={() => handleSubmit(formData)}
      disabled={mutation.isPending}
    >
      {mutation.isPending ? '添加中...' : '添加'}
    </button>
  )
}
```

### 乐观更新

```typescript
export const useSwitchProviderMutation = (appId: AppId) => {
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: async (providerId: string) => {
      return await providersApi.switch(providerId, appId)
    },
    // 乐观更新: 在请求发送前立即更新 UI
    onMutate: async (providerId) => {
      // 取消正在进行的查询
      await queryClient.cancelQueries({ queryKey: ['providers', appId] })

      // 保存当前数据(以便回滚)
      const previousData = queryClient.getQueryData(['providers', appId])

      // 乐观更新
      queryClient.setQueryData(['providers', appId], (old: any) => ({
        ...old,
        currentProviderId: providerId,
      }))

      return { previousData }
    },
    // 如果失败，回滚
    onError: (err, providerId, context) => {
      queryClient.setQueryData(['providers', appId], context?.previousData)
      toast.error('切换失败')
    },
    // 无论成功失败，都重新获取数据
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: ['providers', appId] })
    },
  })
}
```

### 依赖查询

```typescript
// 第二个查询依赖第一个查询的结果
const { data: providers } = useProvidersQuery(appId)
const currentProviderId = providers?.currentProviderId

const { data: currentProvider } = useQuery({
  queryKey: ['provider', currentProviderId],
  queryFn: () => providersApi.getById(currentProviderId!),
  enabled: !!currentProviderId, // 只有当 ID 存在时才执行
})
```

---

## react-hook-form 使用

### 基础表单

```typescript
import { useForm } from 'react-hook-form'
import { zodResolver } from '@hookform/resolvers/zod'
import { z } from 'zod'

// 定义验证 schema
const schema = z.object({
  name: z.string().min(1, '请输入名称'),
  email: z.string().email('邮箱格式不正确'),
  age: z.number().min(18, '年龄必须大于18'),
})

type FormData = z.infer<typeof schema>

function MyForm() {
  const form = useForm<FormData>({
    resolver: zodResolver(schema),
    defaultValues: {
      name: '',
      email: '',
      age: 0,
    },
  })

  const onSubmit = (data: FormData) => {
    console.log(data)
  }

  return (
    <form onSubmit={form.handleSubmit(onSubmit)}>
      <input {...form.register('name')} />
      {form.formState.errors.name && (
        <span>{form.formState.errors.name.message}</span>
      )}

      <button type="submit">提交</button>
    </form>
  )
}
```

### 使用 shadcn/ui Form 组件

```typescript
import { useForm } from 'react-hook-form'
import { zodResolver } from '@hookform/resolvers/zod'
import {
  Form,
  FormControl,
  FormField,
  FormItem,
  FormLabel,
  FormMessage,
} from '@/components/ui/form'
import { Input } from '@/components/ui/input'
import { Button } from '@/components/ui/button'

function MyForm() {
  const form = useForm<FormData>({
    resolver: zodResolver(schema),
  })

  return (
    <Form {...form}>
      <form onSubmit={form.handleSubmit(onSubmit)} className="space-y-4">
        <FormField
          control={form.control}
          name="name"
          render={({ field }) => (
            <FormItem>
              <FormLabel>名称</FormLabel>
              <FormControl>
                <Input placeholder="请输入名称" {...field} />
              </FormControl>
              <FormMessage />
            </FormItem>
          )}
        />

        <Button type="submit">提交</Button>
      </form>
    </Form>
  )
}
```

### 动态表单验证

```typescript
// 根据条件动态验证
const schema = z.object({
  type: z.enum(['official', 'custom']),
  apiKey: z.string().optional(),
  baseUrl: z.string().optional(),
}).refine(
  (data) => {
    // 如果是自定义供应商，必须填写 baseUrl
    if (data.type === 'custom') {
      return !!data.baseUrl
    }
    return true
  },
  {
    message: '自定义供应商必须填写 Base URL',
    path: ['baseUrl'],
  }
)
```

### 手动触发验证

```typescript
function MyForm() {
  const form = useForm<FormData>()

  const handleBlur = async () => {
    // 验证单个字段
    await form.trigger('name')

    // 验证多个字段
    await form.trigger(['name', 'email'])

    // 验证所有字段
    const isValid = await form.trigger()
  }

  return <form>...</form>
}
```

---

## shadcn/ui 组件使用

### Dialog (对话框)

```typescript
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogFooter,
} from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'

function MyDialog() {
  const [open, setOpen] = useState(false)

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>标题</DialogTitle>
          <DialogDescription>描述信息</DialogDescription>
        </DialogHeader>

        {/* 内容 */}
        <div>对话框内容</div>

        <DialogFooter>
          <Button variant="outline" onClick={() => setOpen(false)}>
            取消
          </Button>
          <Button onClick={handleConfirm}>确认</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
```

### Select (选择器)

```typescript
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'

function MySelect() {
  const [value, setValue] = useState('')

  return (
    <Select value={value} onValueChange={setValue}>
      <SelectTrigger>
        <SelectValue placeholder="请选择" />
      </SelectTrigger>
      <SelectContent>
        <SelectItem value="option1">选项1</SelectItem>
        <SelectItem value="option2">选项2</SelectItem>
        <SelectItem value="option3">选项3</SelectItem>
      </SelectContent>
    </Select>
  )
}
```

### Tabs (标签页)

```typescript
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'

function MyTabs() {
  return (
    <Tabs defaultValue="tab1">
      <TabsList>
        <TabsTrigger value="tab1">标签1</TabsTrigger>
        <TabsTrigger value="tab2">标签2</TabsTrigger>
        <TabsTrigger value="tab3">标签3</TabsTrigger>
      </TabsList>

      <TabsContent value="tab1">
        <div>标签1的内容</div>
      </TabsContent>

      <TabsContent value="tab2">
        <div>标签2的内容</div>
      </TabsContent>

      <TabsContent value="tab3">
        <div>标签3的内容</div>
      </TabsContent>
    </Tabs>
  )
}
```

### Toast 通知 (Sonner)

```typescript
import { toast } from 'sonner'

// 成功通知
toast.success('操作成功')

// 错误通知
toast.error('操作失败')

// 加载中
const toastId = toast.loading('处理中...')
// 完成后更新
toast.success('处理完成', { id: toastId })
// 或
toast.dismiss(toastId)

// 自定义持续时间
toast.success('消息', { duration: 5000 })

// 带操作按钮
toast('确认删除?', {
  action: {
    label: '删除',
    onClick: () => handleDelete(),
  },
})
```

---

## 代码迁移示例

### 示例 1: 状态管理迁移

**旧代码** (手动状态管理):

```typescript
const [providers, setProviders] = useState<Record<string, Provider>>({})
const [currentProviderId, setCurrentProviderId] = useState('')
const [loading, setLoading] = useState(false)
const [error, setError] = useState<Error | null>(null)

useEffect(() => {
  const load = async () => {
    setLoading(true)
    setError(null)
    try {
      const data = await window.api.getProviders(appType)
      const currentId = await window.api.getCurrentProvider(appType)
      setProviders(data)
      setCurrentProviderId(currentId)
    } catch (err) {
      setError(err as Error)
    } finally {
      setLoading(false)
    }
  }
  load()
}, [appId])
```

**新代码** (React Query):

```typescript
const { data, isLoading, error } = useProvidersQuery(appId)
const providers = data?.providers || {}
const currentProviderId = data?.currentProviderId || ''
```

**减少**: 从 20+ 行到 3 行

---

### 示例 2: 表单验证迁移

**旧代码** (手动验证):

```typescript
const [name, setName] = useState('')
const [nameError, setNameError] = useState('')
const [apiKey, setApiKey] = useState('')
const [apiKeyError, setApiKeyError] = useState('')

const validate = () => {
  let valid = true

  if (!name.trim()) {
    setNameError('请输入名称')
    valid = false
  } else {
    setNameError('')
  }

  if (!apiKey.trim()) {
    setApiKeyError('请输入 API Key')
    valid = false
  } else if (apiKey.length < 10) {
    setApiKeyError('API Key 长度不足')
    valid = false
  } else {
    setApiKeyError('')
  }

  return valid
}

const handleSubmit = () => {
  if (validate()) {
    // 提交
  }
}

return (
  <form>
    <input value={name} onChange={e => setName(e.target.value)} />
    {nameError && <span>{nameError}</span>}

    <input value={apiKey} onChange={e => setApiKey(e.target.value)} />
    {apiKeyError && <span>{apiKeyError}</span>}

    <button onClick={handleSubmit}>提交</button>
  </form>
)
```

**新代码** (react-hook-form + zod):

```typescript
const schema = z.object({
  name: z.string().min(1, '请输入名称'),
  apiKey: z.string().min(10, 'API Key 长度不足'),
})

const form = useForm({
  resolver: zodResolver(schema),
})

return (
  <Form {...form}>
    <form onSubmit={form.handleSubmit(onSubmit)}>
      <FormField
        control={form.control}
        name="name"
        render={({ field }) => (
          <FormItem>
            <FormControl>
              <Input {...field} />
            </FormControl>
            <FormMessage />
          </FormItem>
        )}
      />

      <FormField
        control={form.control}
        name="apiKey"
        render={({ field }) => (
          <FormItem>
            <FormControl>
              <Input {...field} />
            </FormControl>
            <FormMessage />
          </FormItem>
        )}
      />

      <Button type="submit">提交</Button>
    </form>
  </Form>
)
```

**减少**: 从 40+ 行到 30 行，且更健壮

---

### 示例 3: 通知系统迁移

**旧代码** (自定义通知):

```typescript
const [notification, setNotification] = useState<{
  message: string
  type: 'success' | 'error'
} | null>(null)
const [isVisible, setIsVisible] = useState(false)

const showNotification = (message: string, type: 'success' | 'error') => {
  setNotification({ message, type })
  setIsVisible(true)
  setTimeout(() => {
    setIsVisible(false)
    setTimeout(() => setNotification(null), 300)
  }, 3000)
}

return (
  <>
    {notification && (
      <div className={`notification ${isVisible ? 'visible' : ''} ${notification.type}`}>
        {notification.message}
      </div>
    )}
    {/* 其他内容 */}
  </>
)
```

**新代码** (Sonner):

```typescript
import { toast } from 'sonner'

// 在需要的地方直接调用
toast.success('操作成功')
toast.error('操作失败')

// 在 main.tsx 中只需添加一次
import { Toaster } from '@/components/ui/sonner'

<Toaster />
```

**减少**: 从 20+ 行到 1 行调用

---

### 示例 4: 对话框迁移

**旧代码** (自定义 Modal):

```typescript
const [isOpen, setIsOpen] = useState(false)

return (
  <>
    <button onClick={() => setIsOpen(true)}>打开</button>

    {isOpen && (
      <div className="modal-backdrop" onClick={() => setIsOpen(false)}>
        <div className="modal-content" onClick={e => e.stopPropagation()}>
          <div className="modal-header">
            <h2>标题</h2>
            <button onClick={() => setIsOpen(false)}>×</button>
          </div>
          <div className="modal-body">
            {/* 内容 */}
          </div>
          <div className="modal-footer">
            <button onClick={() => setIsOpen(false)}>取消</button>
            <button onClick={handleConfirm}>确认</button>
          </div>
        </div>
      </div>
    )}
  </>
)
```

**新代码** (shadcn/ui Dialog):

```typescript
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog'

const [isOpen, setIsOpen] = useState(false)

return (
  <>
    <Button onClick={() => setIsOpen(true)}>打开</Button>

    <Dialog open={isOpen} onOpenChange={setIsOpen}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>标题</DialogTitle>
        </DialogHeader>
        {/* 内容 */}
        <DialogFooter>
          <Button variant="outline" onClick={() => setIsOpen(false)}>取消</Button>
          <Button onClick={handleConfirm}>确认</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  </>
)
```

**优势**:
- 无需自定义样式
- 内置无障碍支持
- 自动管理焦点和 ESC 键

---

### 示例 5: API 调用迁移

**旧代码** (window.api):

```typescript
// 添加供应商
const handleAdd = async (provider: Provider) => {
  try {
    await window.api.addProvider(provider, appType)
    await loadProviders()
    showNotification('添加成功', 'success')
  } catch (error) {
    showNotification('添加失败', 'error')
  }
}
```

**新代码** (React Query Mutation):

```typescript
// 在组件中
const addMutation = useAddProviderMutation(appId)

const handleAdd = (provider: Provider) => {
  addMutation.mutate(provider)
  // 成功和错误处理已在 mutation 定义中处理
}
```

**优势**:
- 自动处理 loading 状态
- 统一的错误处理
- 自动刷新数据
- 更少的样板代码

---

## 常见问题

### Q: 如何在 mutation 成功后关闭对话框?

```typescript
const mutation = useAddProviderMutation(appId)

const handleSubmit = (data: Provider) => {
  mutation.mutate(data, {
    onSuccess: () => {
      setIsOpen(false) // 关闭对话框
    },
  })
}
```

### Q: 如何在表单中使用异步验证?

```typescript
const schema = z.object({
  name: z.string().refine(
    async (name) => {
      // 检查名称是否已存在
      const exists = await checkNameExists(name)
      return !exists
    },
    { message: '名称已存在' }
  ),
})
```

### Q: 如何手动刷新 Query 数据?

```typescript
const queryClient = useQueryClient()

// 方式1: 使缓存失效，触发重新获取
queryClient.invalidateQueries({ queryKey: ['providers', appId] })

// 方式2: 直接刷新
queryClient.refetchQueries({ queryKey: ['providers', appId] })

// 方式3: 更新缓存数据
queryClient.setQueryData(['providers', appId], newData)
```

### Q: 如何在组件外部使用 toast?

```typescript
// 直接导入并使用即可
import { toast } from 'sonner'

export const someUtil = () => {
  toast.success('工具函数中的通知')
}
```

---

## 调试技巧

### React Query DevTools

```typescript
// 在 main.tsx 中添加
import { ReactQueryDevtools } from '@tanstack/react-query-devtools'

<QueryClientProvider client={queryClient}>
  <App />
  <ReactQueryDevtools initialIsOpen={false} />
</QueryClientProvider>
```

### 查看表单状态

```typescript
const form = useForm()

// 在开发模式下打印表单状态
console.log('Form values:', form.watch())
console.log('Form errors:', form.formState.errors)
console.log('Is valid:', form.formState.isValid)
```

---

## 性能优化建议

### 1. 避免不必要的重渲染

```typescript
// 使用 React.memo
export const ProviderCard = React.memo(({ provider, onEdit }: Props) => {
  // ...
})

// 或使用 useMemo
const sortedProviders = useMemo(
  () => Object.values(providers).sort(...),
  [providers]
)
```

### 2. Query 配置优化

```typescript
const { data } = useQuery({
  queryKey: ['providers', appId],
  queryFn: fetchProviders,
  staleTime: 1000 * 60 * 5, // 5分钟内不重新获取
  gcTime: 1000 * 60 * 10, // 10分钟后清除缓存
})
```

### 3. 表单性能优化

```typescript
// 使用 mode 控制验证时机
const form = useForm({
  mode: 'onBlur', // 失去焦点时验证
  // mode: 'onChange', // 每次输入都验证(较慢)
  // mode: 'onSubmit', // 提交时验证(最快)
})
```

---

**提示**: 将此文档保存在浏览器书签或编辑器中，方便随时查阅！
