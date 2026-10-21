# axum-openapi — 设计参考文档

> 状态：核心实现已完成，第 13 节验收标准全部通过  
> 目标环境：Rust stable、Axum 0.8.x、OpenAPI 3.1、schemars 1.x

## 1. 项目定位

**axum-openapi** 是一个通过替换路由注册层，为原生 Axum Handler 自动生成 OpenAPI Schema 的 Rust crate。

主要目标：

- **零 Handler 注解**：不要求 `#[utoipa::path]` 等逐路由注解。
- **最小迁移成本**：业务 Handler 不变，主要替换 `Router` 和 `routing::{get,post,...}` 的 import。
- **编译期类型检查**：能够描述的 Extractor 和响应类型由 Trait 约束，避免静默生成错误文档。
- **运行时零额外请求处理成本**：Schema 在应用组装时采集；实际请求由 Axum 原生 `Router` 服务。
- **增量标注**：HTTP 错误码、鉴权、复杂业务语义允许显式补充。

非目标：

- 不承诺从 **已经构建完成** 的 `axum::Router` 反射出完整 OpenAPI 信息。
- 不解析 Handler 函数体、分支与业务验证规则。
- 不承诺仅凭 `IntoResponse` 推断全部运行时状态码。
- 不承诺任意第三方自定义 Extractor 均自动获得精确 Schema。

## 2. 理想使用体验

```rust
use axum::{extract::{Path, Query}, Json};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use axum_openapi::{Router, routing::{get, post}};

#[derive(Serialize, JsonSchema)]
struct User {
    id: u64,
    name: String,
}

#[derive(Deserialize, JsonSchema)]
struct CreateUser {
    name: String,
}

async fn get_user(Path(id): Path<u64>) -> Json<User> {
    Json(User { id, name: "Ada".into() })
}

async fn create_user(Json(input): Json<CreateUser>) -> Json<User> {
    Json(User { id: 1, name: input.name })
}

fn app() -> (axum::Router, axum_openapi::OpenApiDocument) {
    Router::<()>::new()
        .route("/users", post(create_user))
        .route("/users/{id}", get(get_user))
        .finish()
}
```

> 上述代码展示期望的公共 API，不保证与当前草案源码逐字一致。`/users/{id}` 遵循 Axum 0.8 路径语法。

预期文档片段：

```yaml
openapi: 3.1.0
paths:
  /users/{id}:
    get:
      parameters:
        - name: id
          in: path
          required: true
          schema: { type: integer, format: uint64 }
      responses:
        '200':
          description: OK
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/User'
  /users:
    post:
      requestBody:
        required: true
        content:
          application/json:
            schema:
              $ref: '#/components/schemas/CreateUser'
      responses:
        '200':
          description: OK
```

注意：schemars 的数字类型及 `format` 输出以实际转换规则为准，此 YAML 仅用于说明结构。

## 3. 架构

```text
用户 Handler (async fn)
       │
       ▼
axum_openapi::routing::get/post/...
       │
       ├── Handler 签名分析：输入 Extractor、输出类型
       │        │
       │        └── ApiInput / ApiOutput → Operation 元数据
       │
       └── axum::routing::get/post/... → 原生 MethodRouter
                          │
                          ▼
                 axum_openapi::Router::route
                          │
              ┌───────────┴───────────┐
              ▼                       ▼
        原生 axum::Router       OpenAPI Registry
              │                       │
              ▼                       ▼
         HTTP 服务             OpenAPI 3.1 JSON
```

原则：**在注册时采集，而非在 Router 完成后反射。** Axum 路由表以及 `MethodRouter` 的处理器元数据并未提供完整、稳定的公开反射接口。

## 4. 核心数据结构（示意）

```rust
pub struct ApiRouter<S = ()> {
    inner: axum::Router<S>,
    registry: OpenApiRegistry,
}

pub struct ApiMethodRouter<S = ()> {
    inner: axum::routing::MethodRouter<S>,
    operations: Vec<OperationDescriptor>,
}

pub struct OperationDescriptor {
    pub method: http::Method,
    pub parameters: Vec<ParameterDescriptor>,
    pub request_body: Option<RequestBodyDescriptor>,
    pub responses: Vec<ResponseDescriptor>,
    // summary / description / security / tags 等
}
```

`ApiRouter<S>` 的状态泛型应遵循 Axum `Router<S>` 的语义：`S` 表示尚需提供的 state 类型；`with_state` 必须正确改变泛型，不能只原样转发返回值。

### 4.1 路由包装

```rust
pub fn get<H, Sig, S>(handler: H) -> ApiMethodRouter<S>
where
    H: axum::handler::Handler<Sig, S> + AutoHandler<Sig, S>,
    S: Clone + Send + Sync + 'static,
{
    // 先采集元数据，再用 axum::routing::get(handler) 创建真实路由。
    // 实现中应精确检查 Sig/S 的类型参数推断及 trait 约束。
    todo!()
}
```

上面的 API 是约束设计示意，具体泛型签名必须通过 `cargo check` 验证。

## 5. Handler 类型推导

### 5.1 为什么不直接读 `Handler::Future`？

Axum 的 `Handler<T, S>` 关联 Future 最终输出 HTTP `Response`；这已经是经过 `IntoResponse` 转换的统一类型，不能还原最初的 `Json<User>` 或 `Result<Json<User>, ApiError>`。

应在 Handler 被传入 `get()` / `post()` 时，匹配其原始函数签名：

```rust
F: FnOnce(A1, A2, ...) -> Fut,
Fut: Future<Output = R>,
R: ApiOutput,
A1: ApiInput,
A2: ApiInput,
```

### 5.2 `AutoHandler` 设计

建议使用一个**额外签名参数**作为 trait 区分项，避免 0/1/N 参数函数的 blanket impl 发生重叠，同时利用宏生成不同参数个数的实现。

```rust
pub trait AutoHandler<Sig, S>: Sized {
    fn describe() -> OperationDescriptor;
}

// 示意（需要与 Axum 0.8 的 Handler 实现约束对齐）
impl<F, Fut, R, S> AutoHandler<((),), S> for F
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = R>,
    R: ApiOutput,
{
    fn describe() -> OperationDescriptor { todo!() }
}
```

**重要实现约束**：

1. 无参数与多参数签名 marker 的形状必须统一并且不冲突。
2. `Handler` 的最后一个 Extractor 通常满足 `FromRequest<S>`；前面的 Extractor 通常满足 `FromRequestParts<S>`。自定义 Trait 不能误以为所有 Extractor 都可放任意位置。
3. 对原生 Handler 仍加 `axum::handler::Handler<Sig, S>` 约束，让 Axum 检查 `Send`、state 和 Extractor 可用性。
4. `FnOnce` 的泛型推导、返回 Future 的 `Send` 边界、marker 类型与 blanket impl coherence 都必须用最小样例做编译测试。
5. 暂不支持 `HandlerWithoutStateExt`、自定义 Tower `Service`、动态 `route_service` 的自动 Schema 推导。

## 6. Schema Trait

```rust
pub trait ApiInput {
    fn describe_input(ctx: &mut OperationContext);
}

pub trait ApiOutput {
    fn describe_output(ctx: &mut OperationContext);
}
```

### 6.1 输入

| Axum Extractor | OpenAPI 映射 | 注意事项 |
|---|---|---|
| `Json<T>` | `requestBody`，`application/json` | `T: JsonSchema`；放在 Handler 最后 |
| `Query<T>` | `in: query` 参数 | 对象字段展开；数组及深层结构需定义策略 |
| `Path<T>` | `in: path` 参数 | 名称必须从路由模板匹配；tuple / struct 映射需校验 |
| `State<T>` | 无 API 输入 | 不应出现在文档中；仍需兼容 Axum state 提取 |
| `HeaderMap` | 默认不自动声明所有 Header | 可显式标注具体 Header |
| `Extension<T>` | 默认内部依赖 | 按需额外注册 |
| 自定义 Extractor | 扩展点 | 不做无依据的 blanket fallback |

### 6.2 输出

| Handler 返回类型 | OpenAPI 映射 | 注意事项 |
|---|---|---|
| `Json<T>` | 默认 `200 application/json` | `T: JsonSchema` |
| `()` | 默认 `200` 空 body | 后续可自定义 204 |
| `Result<T, E>` | 合并成功与错误响应 | `T`、`E` 都需要可描述；必须避免 impl 冲突 |
| `(StatusCode, Json<T>)` | body 可推导 | **状态码值不可从类型确定** |
| `impl IntoResponse` | 通常无法自动得知具体响应类型 | 可以使用响应新类型或手工描述 |
| 自定义 `ApiError` | 建议实现 `ApiOutput` 或 derive | 状态码可能由不同变体决定 |

**不能仅靠函数签名推断**：某个业务条件下返回 400/404/409、鉴权策略、运行时动态状态码、分页语义、错误详情等。

## 7. 路径与 Query 映射

- Axum 0.8 使用 `/{id}`、`/{category}/{id}` 格式。
- 对 `Path<u64>`：从模板中取得唯一的 `{id}` 参数名；若出现多个参数则应报配置错误。
- 对 `Path<(u64, String)>`：按模板参数顺序映射类型。
- 对 `Path<MyStruct>`：按结构字段名与路径模板对齐，检测缺失或多余字段。
- `Query<T>` 以对象字段为默认模型；至少支持简单标量和可选字段。
- `Query<T>` 的 `Vec`、flatten、rename、深层对象、style/explode，需要独立规则和测试。

## 8. Router API 与元数据合并

优先实现：

```rust
Router::new()
    .route("/users", get(list_users).post(create_user))
    .nest("/v1", v1_router)
    .merge(other_router)
    .with_state(state)
    .finish();
```

实现规则：

- `route` 同时注册底层 Axum MethodRouter 和 OpenAPI operations。
- `nest` 需要为文档路径统一加前缀，并遵守 Axum 对特殊路径、尾斜杠等行为。
- `merge` 合并元数据，检测重复 `(path, method)`。
- `with_state` 保留文档元数据，并正确处理 `Router<S>` 的泛型变化。
- `fallback`、`route_service`、`layer`、`method_not_allowed_fallback` 第一阶段只转发或明确不支持自动文档化。
- `finish` 输出 `(axum::Router, OpenApiDocument)`；是否暴露原始 JSON 应是单独的方法。

## 9. Schema 注册表

MVP 可先 inline Schema，但正式版本应：

1. 使用 `schemars` 生成 JSON Schema。
2. 将复用的模型存入 `components.schemas`。
3. 合并 `$defs` 和递归引用，重新定位 `$ref`。
4. 按确定性顺序输出，方便 Git diff。
5. 对同名不同结构的模型检测冲突。
6. 添加兼容 OpenAPI 3.1 / JSON Schema 2020-12 的测试。

不建议只把 `$defs` 直接搬到 `components.schemas`：必须同步更新内部引用和递归引用。

## 10. MVP 范围与交付顺序

### Milestone 0：类型系统可行性验证

创建最小 crate，只验证：

- `get(no_args)` 能获取 `Json<User>` 输出类型。
- `post(Json<CreateUser>)` 能获取请求与响应类型。
- `get(Path<u64>)` 能获取路径提取器类型。
- 相同函数签名能被 Axum `Handler<Sig, S>` 接受。
- rustc 不出现 `E0119`（冲突实现）与 `E0207`（无约束泛型）。

**先完成这一步再设计完整 Router。**

### Milestone 1：基础功能

- `get` / `post` 与 `ApiMethodRouter::post` 链式注册。
- `Router::new` / `route` / `finish`。
- `Json<T>` 请求和响应。
- `Path<T>` 的简单标量。
- 生成合法 OpenAPI 3.1 JSON。
- 单元测试校验路由运行和 Schema 输出。

### Milestone 2：路由与模型

- `Query<T>` 简单结构字段展开。
- `nest` / `merge` / `with_state`。
- `components.schemas` 去重。
- 0–16 个 Handler 参数宏生成。

### Milestone 3：真实项目能力

- `Result<T, E>`、常用 Response tuple。
- 显式 `.response::<T>(status)`、`.summary(...)`、`.security(...)`。
- 自定义 Extractor / Error 扩展。
- OpenAPI 文档路由及 Swagger UI 集成（可选 Feature；当前未提供 Scalar 集成）。

## 11. 建议 crate 结构

```text
axum-openapi/
├── Cargo.toml
├── src/
│   ├── lib.rs
│   ├── router.rs          # Router / ApiMethodRouter
│   ├── routing.rs         # get / post / put / delete ...
│   ├── handler.rs         # AutoHandler 与不同参数数量的宏
│   ├── extract.rs         # ApiInput / Path / Query / Json / State
│   ├── response.rs        # ApiOutput
│   ├── operation.rs       # OperationDescriptor
│   ├── params.rs          # Path / Query 参数渲染与校验
│   ├── schema.rs          # schemars + 组件注册表
│   └── openapi.rs         # OpenAPI 3.1 输出
├── tests/
│   ├── common/mod.rs      # 共享模型与测试 Router
│   ├── openapi.rs         # 文档结构断言
│   ├── http.rs            # tower::ServiceExt 请求测试
│   ├── validation.rs      # oas3 解析校验 + $ref 可解析性
│   ├── compile.rs         # trybuild 编译失败诊断
│   ├── ui/                # 编译失败用例与预期 stderr
│   ├── swagger.rs         # Swagger UI 测试
│   ├── golden.rs          # JSON 快照测试
│   └── fixtures/basic.json
└── examples/
    └── basic.rs
```

## 12. 技术风险清单

| 风险 | 影响 | 处理方式 |
|---|---|---|
| Handler impl 与自定义签名 Trait 不一致 | 编译失败 | 与 Axum 0.8 源码约束逐项对齐、编译测试 |
| 泛型 blanket impl 重叠 | E0119 | 使用不同 marker 形状，避免无约束泛型 |
| `impl IntoResponse` 隐藏具体类型 | 不能自动展开返回结构 | 响应包装类型或手工补充 |
| 自定义 Extractor 无静态语义 | 文档缺失 | 插件式 `ApiInput` 扩展，编译期明确失败 |
| 路由模板与 Path 字段不匹配 | 错误参数文档 | 注册时校验并返回错误 |
| Schema 转换的 `$ref` 出错 | 客户端生成器不兼容 | Golden test + OpenAPI validator |
| Axum 小版本内部行为变化 | 兼容性退化 | 只依赖公开 API；明确支持版本范围 |

## 13. 验收标准

- [x] Handler 无 OpenAPI 注解。
- [x] 替换 Router/routing imports 即可运行常见 JSON API。
- [x] `cargo check --all-targets` 通过。
- [x] `cargo test` 通过。
- [x] `get(Json)` 和 `post(Json)` 类型提取正确。
- [x] `Path` 字段名与 `/{param}` 对应。
- [x] 生成文档通过 OpenAPI 3.1 校验。
- [x] 真实 HTTP 路由行为与原生 Axum 一致。
- [x] 未支持类型的编译错误可理解，不会默默省略。
- [x] 复杂状态码/错误响应可手动补全。

验证结果（2026-10-22）：

| 验收项 | 证据 |
|---|---|
| 无注解、import 替换、常见 JSON API | `examples/basic.rs`、`tests/http.rs`、`tests/golden.rs` |
| 编译与测试 | `cargo check --all-targets --all-features`、`cargo test --all-features` 通过（单元 11、HTTP 11、文档 25、校验 2、编译失败用例 2、Swagger 2、golden 1、doctest 4） |
| Path 字段与模板对应 | `tests/openapi.rs` 中的标量/元组/结构体/通配符用例；字段不匹配时注册阶段 panic |
| OpenAPI 3.1 校验 | `tests/validation.rs` 使用 `oas3` 完整解析文档并校验版本与所有内部 `$ref`；`tests/fixtures/basic.json` 另经 Redocly `struct` / `no-unresolved-refs` 规则校验通过 |
| 真实 HTTP 行为 | `tests/http.rs` 通过 `tower::ServiceExt::oneshot` 覆盖路径、Query、JSON body、状态码、错误分支、State、嵌套/合并与未知路由 |
| 编译错误可理解 | `tests/compile.rs` 使用 trybuild 固化 `tests/ui/*.stderr`；`AutoHandler` 的 `#[diagnostic::on_unimplemented]` 明确指出 `ApiInput` / `ApiOutput` 约束与常见原因 |
| 手工补全状态码/错误响应 | `ApiMethodRouter` 的 `response` / `response_with` / `error_response` / `empty_response` / `raw_response`，由 `tests/openapi.rs` 覆盖 |

## 14. 参考资料

- Axum 文档：https://docs.rs/axum/latest/axum/
- Axum 源码：https://github.com/tokio-rs/axum
- schemars：https://github.com/GREsau/schemars
- aide：https://github.com/tamasfe/aide
- utoipa：https://github.com/juhaku/utoipa
- OpenAPI 3.1 规范：https://spec.openapis.org/oas/v3.1.0.html

---

**决策建议：** 先写一份 100–200 行的 Handler 类型推导编译实验，确认 `FnOnce(...) -> Fut`、`Future<Output=R>`、Axum `Handler<Sig,S>` 可以同时表达；一旦成功，再推进 Router 包装与 Schema 注册表。不要首先做 UI、宏或文档序列化细节。
