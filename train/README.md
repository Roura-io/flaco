# flacoAi training — hackathon 3

Goal: a LoRA adapter on top of `qwen2.5-coder:7b-instruct` trained on
Chris's SwiftUI / Swift 6 / POP architecture style and Walter-voice
examples, served back to the orchestrator as `flaco-custom:7b`.

## Directory layout

```
train/
├── README.md                  # you are here
├── rubric.md                  # the architecture rubric — gate 1 review
├── colab/
│   └── flaco_train.ipynb      # click "Run all" in Google Colab
├── data/
│   ├── raw/                   # scraped source (gitignored)
│   ├── pairs/                 # generated JSONL training pairs (gitignored)
│   ├── seeds/                 # hand-written seed examples
│   └── eval/
│       └── holdout.jsonl      # curated eval cases (scored against rubric)
├── scripts/
│   ├── harvest_riokit.py      # extract positive examples from RIOExperimentationKit
│   ├── harvest_luminae.py     # extract positive examples from the Luminae iOS repo
│   ├── harvest_memory.py      # pull user/Walter facts from ~/infra/flaco.db
│   ├── synthesize_pairs.py    # use local qwen3:32b to expand seeds → training pairs
│   ├── build_dataset.py       # merge raw + synthetic → final JSONL
│   └── eval_model.py          # run eval cases against any Ollama model
└── ollama/
    └── Modelfile              # Ollama recipe to serve the LoRA adapter
```

## Pipeline

```
 RIOExperimentationKit  ──┐
 Luminae iOS repo        ──┼──► harvesters ──► raw/ ─┐
 flaco memory / Jira     ──┘                          │
                                                      ├──► build_dataset ──► pairs/train.jsonl
 seed interview (Chris)  ──► seeds/*.jsonl ──► synth ─┘                              │
                                                                                     ▼
                                                                              Colab LoRA training
                                                                                     │
                                                                                     ▼
                                                                             adapter + GGUF
                                                                                     │
                                                                                     ▼
                                                                       Ollama → flaco-custom:7b
                                                                                     │
                                                                                     ▼
                                                                       flaco-core router
```

## Gates (where I stop and ask for you)

1. **Rubric review** — read `rubric.md`, mark corrections, approve.
2. **Seed interview** — I DM you ~40 prompts. You free-form answer.

Everything else I run myself. See `GRADING3.md` for the reviewer guide.

## Running it yourself

```bash
# 1. Harvest
python3 scripts/harvest_riokit.py
python3 scripts/harvest_luminae.py
python3 scripts/harvest_memory.py

# 2. Synthesize (runs on your local Ollama qwen3:32b ~5 hours)
python3 scripts/synthesize_pairs.py

# 3. Build the final JSONL
python3 scripts/build_dataset.py

# 4. Open colab/flaco_train.ipynb in Google Colab
#    Upload data/pairs/train.jsonl to Colab session
#    Runtime → Change type → T4 GPU
#    Runtime → Run all

# 5. Download the trained adapter
#    Place adapter and Modelfile in ~/infra/flaco-custom/
#    Register with: ollama create flaco-custom:7b -f Modelfile
```

## Why Colab and not Mac Studio

The M3 Ultra is still shipping — mid-May. Until then:
- **Primary**: Google Colab free T4 (12 GB VRAM, ~4-6 hour sessions).
  Enough for LoRA on 7B models with QLoRA.
- **Backup**: Kaggle free tier (T4 x2, 30 h/week, longer sessions).
- **When M3 arrives**: retrain on the Mac via MLX with higher LoRA
  rank and more epochs. The pipeline is designed so this is literally
  "swap the training cell and re-run."

Budget: **$0**.
