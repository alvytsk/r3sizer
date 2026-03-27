# r3sizer / imgsharp — обзор современных подходов к уменьшению фотографий и roadmap

## 1. Цель документа

Этот документ собирает в одном месте:

- краткий обзор **актуальных подходов** к уменьшению фотографий с сохранением визуального качества;
- выводы, которые **полезны именно для `r3sizer` / `imgsharp-core`**;
- практический **roadmap для production-grade Rust core**;
- явное разделение на:
  - **подтвержденное источниками**;
  - **инженерные выводы / inferred design**;
  - **экспериментальные направления**.

Документ ориентирован на текущий вектор проекта:

- сначала **Rust core**;
- затем CLI;
- затем Tauri GUI;
- без ложной уверенности в деталях исходных статей;
- с упором на **deterministic pipeline**, диагностику и расширяемость.

---

## 2. Краткий вывод

На текущий момент поле downscaling развивается не в сторону одного нового «идеального фильтра», а в сторону более широкой постановки задачи:

1. **Оценка качества downscaling** становится такой же важной, как и сам алгоритм.
2. Современные работы пытаются учитывать не только blur, но и более широкий набор эффектов: **aliasing, ringing, halo, texture loss, perceptual preference**.
3. Есть активная линия **learned / invertible rescaling**, но она часто оптимизирует не просто красивый уменьшенный кадр, а LR-представление, удобное для последующего восстановления.
4. Есть усиление интереса к **RAW-domain / early-pipeline downscaling**, что хорошо сочетается с твоим выбором делать вычисления в **linear space**.
5. Для `r3sizer` наиболее разумный путь — **не перепрыгивать сразу в end-to-end ML**, а усилить классический pipeline более сильной метрикой, robust solver-логикой, диагностикой и осторожной адаптацией по содержимому изображения.

---

## 3. Что считается подтвержденным

### 3.1. Downscaling — это уже не просто выбор interpolation kernel

Классические работы давно показали, что визуально хороший ресайз требует не только интерполяции, но и:

- адаптации к локальному содержимому изображения;
- осознанной работы с деталями;
- контроля перцептуальных артефактов.

Это важно как фон: современные исследования в основном развивают эти же идеи, а не отменяют их.

### 3.2. Оценка качества downscaling стала отдельной исследовательской задачей

Работы уровня **IDA-RD (CVPR 2024)** показывают, что качество после downscaling плохо описывается только традиционными метриками вроде PSNR/SSIM. Вместо этого предлагаются более специализированные постановки, где учитывается, насколько сам процесс downscaling искажает визуально важную информацию.

**Практический смысл для проекта:** текущая функция артефактов не должна считаться полной моделью качества.

### 3.3. У изображения может быть свой «лучший» масштаб

Работы по **Image Intrinsic Scale / Image Intrinsic Scale Assessment (IIS / IISA, 2025)** подталкивают к мысли, что не всегда «больше деталей = субъективно лучше». Визуальный оптимум может существовать на некотором промежуточном масштабе.

**Практический смысл для проекта:** качество после уменьшения не обязано вести себя монотонно и не сводится к минимизации blur.

### 3.4. Современные работы явно работают с компромиссом detail vs artifacts

Работы вроде **DCID (2024)** прямо строятся вокруг trade-off между усилением деталей и подавлением артефактов.

**Практический смысл для проекта:** идея `artifact-limited sharpening` не устарела. Она хорошо совпадает с реальной проблемой, которую решают современные методы.

### 3.5. Есть отдельная сильная линия structure-aware / co-occurrence-aware downscaling

Работы 2023–2025 годов по co-occurrence / structure-aware downscaling усиливают акцент на сохранении структур и тонкой фактуры без грубого edge blur.

**Практический смысл для проекта:** локальная адаптация и region-aware логика — перспективное развитие mainline.

### 3.6. Усилился интерес к RAW-domain downscaling

Свежие работы 2025 года говорят о том, что обычный sRGB-domain downscaling может терять детали и вносить blur / ghosting / color distortion. Поэтому часть исследований уходит к RAW-domain или к более ранним стадиям pipeline.

**Практический смысл для проекта:** решение работать в **linear RGB / floating point** выглядит концептуально сильным и согласованным с направлением поля.

### 3.7. Есть активная ветка invertible / learned rescaling

Работы типа **IRN, AIDN, HCD, T-InvBlocks** трактуют downscaling как процесс, где уменьшенное изображение должно быть не только приятным визуально, но и сохранять информацию для последующего восстановления.

**Практический смысл для проекта:** это полезный источник идей, но это не автоматически лучший baseline для standalone photo downscaling library.

---

## 4. Что важно для `r3sizer` именно сейчас

### 4.1. Текущий baseline жизнеспособен

Текущая практическая схема:

1. загрузка raster image;
2. переход из nonlinear RGB в linear RGB;
3. downscale в linear space;
4. optional contrast leveling;
5. probing нескольких значений sharpness;
6. расчет `P(s)`;
7. cubic fit для `P(s)`;
8. поиск `s*` под целевой порог `P0`;
9. применение sharpening;
10. обратное преобразование в output color space.

Это **не выглядит устаревшим**. Напротив, такая схема хорошо ложится в deterministic engineering pipeline для production-grade Rust core.

### 4.2. Но текущая метрика слишком узкая, чтобы быть финальной

Текущее определение:

```text
P(s) = proportion of linear RGB channel values outside [0, 1] after sharpening
```

Это хороший **engineering approximation**, но не полная модель визуальных артефактов.

#### Что она реально хорошо ловит

- clipping / overshoot;
- опасные зоны после sharpening;
- выход за допустимый диапазон при работе в linear float.

#### Что она почти не ловит

- ringing без явного выхода за диапазон;
- halo вокруг границ;
- texture flattening;
- aliasing;
- субъективно неприятную «перехрусткость».

**Вывод:** эту метрику надо оставить, но понизить её статус до `metric_v0`, а не считать конечной моделью качества.

---

## 5. Рекомендуемый подход для mainline

### 5.1. Главная идея

Для mainline стоит делать ставку не на «новый чудо-kernel» и не на быстрый прыжок в end-to-end ML, а на:

- **чистую и детерминированную архитектуру**;
- **расширяемую систему метрик**;
- **robust solver и fallback-логику**;
- **сильную диагностику**;
- позже — **осторожную content-adaptive sharpening логики**.

Это лучше всего соответствует:

- текущему состоянию проекта;
- будущей интеграции в CLI и Tauri;
- необходимости production-grade поведения;
- требованию не выдавать неподтвержденные детали исходных статей за факт.

### 5.2. Что не стоит делать ядром первой production-версии

Не стоит строить первый mainline вокруг:

- full learned rescaling;
- invertible neural LR representation;
- тяжелой ML-инференсной зависимости;
- полного RAW-pipeline с demosaic-aware логикой.

Это все может быть полезно как research branch, но не как ближайший основной путь.

---

## 6. Архитектурный вектор для `imgsharp-core`

### 6.1. Базовые модули

Рекомендуемая модульная декомпозиция:

- `color`
- `resize`
- `sharpen`
- `metrics`
- `fit`
- `solver`
- `pipeline`
- `diagnostics`

### 6.2. Принципы

- deterministic behavior;
- processing in linear color space;
- floating-point intermediates;
- explicit diagnostics;
- no fake certainty about source material;
- modularity before premature optimization.

### 6.3. Suggested workspace direction

- `imgsharp-core` — algorithms and pipeline
- `imgsharp-io` — loading/saving and buffer conversion
- `imgsharp-cli` — command line interface and batch sweeps
- `imgsharp-tauri` — future GUI shell

---

## 7. Практический roadmap

# 7.1. Этап `v0.1` — Solid baseline

### Цель

Сделать baseline **строго воспроизводимым, тестируемым и исследуемым**.

### Что входит

#### 1. Reference pipeline

Зафиксировать canonical pipeline:

1. decode / import
2. nonlinear RGB → linear RGB
3. resize in linear space
4. optional contrast stage
5. probe sharpness strengths
6. evaluate metric
7. cubic fit
8. solve for `s*`
9. final sharpening
10. encode / export

#### 2. Текущую метрику формализовать как `metric_v0`

Название:

```text
artifact_metric_v0_gamut_excursion
```

И явно документировать как **engineering approximation**.

#### 3. Robust solver instead of naive fit

Нужны следующие проверки:

- monotonicity / quasi-monotonicity of sampled `P(s)`;
- fit residuals;
- root existence in valid interval;
- stability under leave-one-out check;
- safe fallback when all samples exceed budget.

#### 4. Типизированные fallback reasons

Вместо одного общего fallback:

- `NoSampleWithinBudget`
- `FitUnstable`
- `RootOutOfRange`
- `MetricNonMonotonic`
- `BudgetTooStrictForContent`

#### 5. JSON diagnostics

Для каждого прогона сохранять:

- selected strength;
- target metric;
- measured metric;
- all probes;
- fit quality;
- fallback reason;
- timings;
- output size.

#### 6. Dataset sweep mode в CLI

Режим пакетного прогона по датасету с агрегированным summary.

### Критерии готовности

- стабильное поведение на одном и том же input/config;
- понятная диагностика неудач solver;
- возможность сравнивать стратегии не глазами, а через сохраненные отчеты.

---

# 7.2. Этап `v0.2` — Better metrics, same core philosophy

### Цель

Усилить качество выбора `s*` без отказа от deterministic pipeline.

### Что входит

#### 1. Ввести `metric_v1` как составную proxy-metric

Предлагаемая форма:

```text
P_total(s) =
    w1 * gamut_excursion
  + w2 * halo_ringing
  + w3 * edge_overshoot
  + w4 * texture_flattening
```

#### 2. Компоненты метрики

##### `gamut_excursion`
Оставить текущую компоненту.

##### `halo_ringing`
Оценивать аномальные колебания около сильных границ.

##### `edge_overshoot`
Сравнивать локальный контраст и выход за разумный edge envelope.

##### `texture_flattening`
Оценивать потерю микро-фактуры или, наоборот, её неестественное перешарпливание.

#### 3. Luma-aware evaluation

Не переводить весь pipeline в новый color model, но добавить возможность:

- оценивать некоторые компоненты по `luma`;
- отдельно анализировать RGB и luminance behavior.

#### 4. Richer reports

Добавить в diagnostics component-wise scores.

### Критерии готовности

- выбор `s*` меньше зависит от одной clipping-like характеристики;
- появляется более правдоподобное различение между blur / halo / harsh sharpening.

---

# 7.3. Этап `v0.3` — Content-adaptive behavior

### Цель

Сделать sharpening более осмысленным относительно содержимого изображения.

### Что входит

#### 1. Region classification

Классы зон:

- flat regions
- textured regions
- strong edges
- fine detail / microtexture
- risky halo zones

#### 2. Local sharpening gain

Вместо одного глобального коэффициента:

```text
s_local(x, y) = s_global * region_gain(class(x, y))
```

Примерно:

- flat: меньше sharpen;
- strong edges: нормальный sharpen;
- risky halo zones: safer sharpen;
- microtexture: умеренное усиление.

#### 3. Contrast leveling as explicit strategy

Вместо неформального optional stage:

- `NoContrastLeveling`
- `LocalContrastCompression`
- `LumaOnlyMicrocontrast`
- `EdgeAwareContrastLeveling`

И четко документировать, что это **inferred engineering design**, а не подтвержденный порядок из исходных статей.

### Критерии готовности

- меньше ореолов и грубого перешарпа на сложных изображениях;
- лучшее поведение на foliage, fabric, architecture, high-contrast scenes.

---

# 7.4. Этап `v0.4` — Experimental branch

### Цель

Добавить исследовательские направления, не ломая mainline.

### Возможные ветки

#### A. Learned evaluator

Не learned resizer, а learned predictor, который помогает оценить качество downscaling или выбрать `s*`.

#### B. Region-adaptive resize kernels

Вдохновляться structure-aware / co-occurrence-aware линией.

#### C. RAW-friendly ingress

Не full RAW pipeline сразу, а возможность принимать данные ближе к ранней стадии pipeline.

#### D. Alternative color handling

Эксперименты с:

- `RgbIndependent`
- `LumaOnly`
- `LumaPlusChromaGuard`

### Что не делать в mainline без отдельного исследования

- full IRN / AIDN style invertible rescaling as the main approach;
- end-to-end neural downsizer as core dependency;
- implicit black-box metric without diagnostics.

---

## 8. Что должно быть в evaluation harness

Это обязательная часть roadmap.

### 8.1. Категории изображений

Набор изображений должен включать:

- portraits
- architecture
- foliage / trees / grass
- fine fabric / texture
- high-contrast graphics
- night scenes
- smartphone-noisy photos
- JPEG-compressed photos

### 8.2. Что сохранять на каждый прогон

- input path / id
- output path / id
- target size
- resize strategy
- sharpen strategy
- selected strength
- probe strengths and probe results
- metric components
- fit summary
- fallback reason
- processing time
- optional debug maps

### 8.3. Что важно сравнивать

- стабильность solver;
- качество выбора `s*`;
- поведение на разных типах сцен;
- чувствительность к уменьшению в 2x / 4x / arbitrary scale.

---

## 9. Что нужно от `imgsharp-cli`

CLI должен быть не просто «оберткой над core», а полноценным исследовательским инструментом.

### Рекомендуемые режимы

- `run` — обычная обработка
- `probe` — вывод всех sampled strengths и metric values
- `compare` — сравнение стратегий
- `dump-debug` — сохранить промежуточные отчеты и карты
- `sweep` — пакетный прогон по датасету

### Пример структуры JSON-отчета

```json
{
  "selected_strength": 0.18,
  "target_metric": 0.001,
  "measured_metric": 0.00093,
  "fit_used": true,
  "fallback_used": false,
  "metric_components": {
    "gamut_excursion": 0.00041,
    "halo": 0.00022,
    "edge_overshoot": 0.00018,
    "texture_flattening": 0.00012
  }
}
```

Это даст почти готовую основу для будущего Tauri diagnostics panel.

---

## 10. Приоритеты реализации

### Приоритет 1 — сделать baseline измеримым

Сначала нужны:

- reference pipeline;
- reproducibility;
- solver diagnostics;
- dataset sweeps;
- типизированные fallback reasons.

### Приоритет 2 — усилить метрику

Следующий главный шаг:

- перейти от одной clipping-like меры к `metric_v1`.

### Приоритет 3 — только потом адаптировать поведение к контенту

После того как качество выбора `s*` станет хорошо диагностируемым:

- region classification;
- local sharpening gains;
- contrast strategies.

### Приоритет 4 — отдельно исследовать advanced branches

Отдельно от mainline:

- learned evaluator;
- adaptive kernels;
- RAW-friendly ingress;
- alternative color/sharpen paths.

---

## 11. Confirmed vs inferred vs experimental

## 11.1. Confirmed by sources

### Confirmed

- Downscaling quality не сводится к простым традиционным метрикам.
- Перцептуальное качество зависит не только от blur, но и от aliasing / ringing / related artifacts.
- Есть активные исследования structure-aware / co-occurrence-aware downscaling.
- Есть активные исследования RAW-domain downscaling.
- Есть активная ветка invertible / learned rescaling.

## 11.2. Inferred engineering design for this project

### Inferred

- Mainline проекта лучше строить вокруг deterministic pipeline, а не вокруг end-to-end neural resizer.
- Следующий strongest step — это richer metric system, а не новый resize kernel.
- Content-adaptive sharpening логично добавлять после усиления diagnostics и solver quality.
- Contrast leveling нужно оставить отдельным явным модулем-стратегией.

## 11.3. Experimental / not yet confirmed for the source papers

### Experimental / not confirmed

- exact resize kernel from the original papers;
- exact sharpening formula from the original author;
- exact original definition of out-of-gamut / artifact metric;
- exact role and order of contrast leveling in the original method;
- exact probe sampling strategy used in the paper.

Эти пункты нельзя выдавать как подтвержденные. Их нужно продолжать маркировать как неизвестные или инженерно выведенные.

---

## 12. Рекомендуемый следующий practical step

Если переводить roadmap в ближайший actionable технический шаг, то самым сильным следующим milestone будет:

### `v0.1 -> v0.2 bridge`

1. formalize `metric_v0`;
2. improve solver diagnostics;
3. add sweep/report infrastructure;
4. implement first version of `metric_v1`;
5. compare `metric_v0` vs `metric_v1` on a small curated dataset.

Это даст максимальный прирост качества решения при минимальном архитектурном риске.

---

## 13. Sources / reading list

Ниже — источники, на которых основан обзор и roadmap.

### Classical / background

1. Johannes Kopf et al. — **Content-Adaptive Image Downscaling**  
   https://johanneskopf.de/publications/downscaling/

### Downscaling assessment / perceptual framing

2. Liang et al. — **Deep Generative Model based Rate-Distortion for Image Downscaling Assessment (CVPR 2024)**  
   https://openaccess.thecvf.com/content/CVPR2024/papers/Liang_Deep_Generative_Model_based_Rate-Distortion_for_Image_Downscaling_Assessment_CVPR_2024_paper.pdf

3. Hosu et al. — **Image Intrinsic Scale Assessment: Bridging the Gap Between Quality and Preference for Image Downscaling (ICCV 2025)**  
   https://openaccess.thecvf.com/content/ICCV2025/papers/Hosu_Image_Intrinsic_Scale_Assessment_Bridging_the_Gap_Between_Quality_and_ICCV_2025_paper.pdf

### Trade-off and region-aware ideas

4. DCID — **A Divide-and-Conquer Approach to Solving the Trade-Off Problem between Enhancement and Artifact Generation in Image Downscaling**  
   https://pure.dongguk.edu/en/publications/dcid-a-divide-and-conquer-approach-to-solving-the-trade-off-probl/

### Structure-aware / co-occurrence-aware line

5. **Image Downscaling Based on Co-Occurrence Learning** (2023)  
   https://www.sciencedirect.com/science/article/abs/pii/S1047320323000160

### Invertible / learned rescaling

6. Xiao et al. — **Invertible Image Rescaling**  
   https://arxiv.org/abs/2005.05650

7. **T-InvBlocks / low-frequency YCbCr branch for image rescaling**  
   https://arxiv.org/abs/2412.13508

### RAW-domain direction

8. **Learning Arbitrary-Scale RAW Image Downscaling**  
   https://arxiv.org/abs/2507.23219

### Original inspiration-related article

9. Sverdlov — **Automatic sharpness adjustment in the reduction of the digital image size**  
   https://novtex.ru/prin/eng/10.17587/prin.9.140-144.html

---

## 14. Final recommendation

Для `r3sizer` / `imgsharp` наиболее сильная стратегия сейчас выглядит так:

- сохранить текущий deterministic linear-space pipeline как основу;
- формально отделить confirmed facts от engineering approximations;
- быстро усилить solver и diagnostics;
- затем перейти к составной proxy-metric;
- только потом добавлять content-adaptive logic;
- ML / RAW / invertible направления держать в experimental branch, пока не появится достаточно инфраструктуры для честной оценки выигрыша.

Это наиболее реалистичный путь к **production-grade Rust image processing core**, который потом естественно расширится в CLI и Tauri UI.
