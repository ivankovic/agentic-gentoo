#!/usr/bin/env python3
"""LoRA fine-tune of a small model on exported drill repairs (export.py), on one small GPU.

    train.py TRAIN.jsonl --model Qwen/Qwen3.5-2B --out DIR [--hold-out DRILL ...]
             [--epochs 3] [--max-tokens 4096] [--qlora]

The model learns the assistant turns only (its tool calls and final answers); the system prompt,
the task and the command outputs are context. Examples longer than --max-tokens are cut at the
end, which keeps their early turns as they are. Repairs of --hold-out drills are not trained on;
they are the validation loss, and the drills to rescore for whether the model learned the skill
rather than the drill.

Written for a 6 GB Pascal card (no bf16, no FlashAttention): fp32 compute, NF4 base with --qlora,
gradient checkpointing, and the output layer's loss a slice at a time. DIR gets the adapter after
each epoch (DIR/epoch-N) and a log of the losses (DIR/log.jsonl).
"""
import argparse
import json
import math
import random
import time
from pathlib import Path

import torch
import torch.nn.functional as Fn
from peft import LoraConfig, get_peft_model
from torch.utils.checkpoint import checkpoint
from transformers import AutoModelForCausalLM, AutoTokenizer, BitsAndBytesConfig

CHUNK = 256  # tokens per slice of the output layer: the full-vocabulary scores never exist at once
ASSISTANT = "<|im_start|>assistant\n"
END = "<|im_end|>"
EMPTY_THINK = "<think>\n\n</think>\n\n"


def as_template(messages):
    """Tool call arguments as objects, which chat templates iterate; the export has JSON strings."""
    out = []
    for m in messages:
        m = dict(m)
        if m.get("tool_calls"):
            m["tool_calls"] = [{**c, "function": {**c["function"], "arguments": json.loads(a) if isinstance(
                a := c["function"]["arguments"], str) else a}} for c in m["tool_calls"]]
        out.append(m)
    return out


def encode(tok, rec, max_tokens):
    """Token ids and the mask of the tokens to learn: each assistant turn's text up to and with its
    end marker. The empty thinking block is left out, since the prompt supplies it at inference."""
    text = tok.apply_chat_template(as_template(rec["messages"]), tools=rec["tools"], tokenize=False,
                                   enable_thinking=False)
    spans, i = [], 0
    while (s := text.find(ASSISTANT, i)) >= 0:
        s += len(ASSISTANT)
        if text.startswith(EMPTY_THINK, s):
            s += len(EMPTY_THINK)
        e = text.find(END, s)
        e = len(text) if e < 0 else e + len(END)
        spans.append((s, e))
        i = e
    enc = tok(text, return_offsets_mapping=True, add_special_tokens=False)
    ids, offsets = enc["input_ids"][:max_tokens], enc["offset_mapping"][:max_tokens]
    learn = [any(s <= a and b <= e for s, e in spans) and b > a for a, b in offsets]
    return torch.tensor(ids), torch.tensor(learn)


def loss_sum(model, ids, learn):
    """Summed next-token loss over the learned tokens, the output layer a slice at a time."""
    base = model.get_base_model()
    h = base.model(input_ids=ids[None].cuda()).last_hidden_state[0, :-1]
    y = ids[1:].cuda().masked_fill(~learn[1:].cuda(), -100)
    head = base.get_output_embeddings()

    def part(h, y):
        return Fn.cross_entropy(head(h).float(), y, reduction="sum", ignore_index=-100)

    return sum(checkpoint(part, h[i:i + CHUNK], y[i:i + CHUNK], use_reentrant=False)
               for i in range(0, y.numel(), CHUNK) if (y[i:i + CHUNK] >= 0).any())


def load(name, qlora, rank):
    kw = dict(attn_implementation="sdpa", device_map={"": 0}, dtype=torch.float32)
    if qlora:
        kw["quantization_config"] = BitsAndBytesConfig(
            load_in_4bit=True, bnb_4bit_quant_type="nf4",
            bnb_4bit_compute_dtype=torch.float32, bnb_4bit_use_double_quant=True)
    m = AutoModelForCausalLM.from_pretrained(name, **kw)
    m.gradient_checkpointing_enable(gradient_checkpointing_kwargs={"use_reentrant": False})
    m.enable_input_require_grads()
    m.config.use_cache = False
    cfg = LoraConfig(r=rank, lora_alpha=2 * rank, lora_dropout=0.0, target_modules="all-linear",
                     task_type="CAUSAL_LM")
    # Training mode: in eval mode transformers skips gradient checkpointing without a word.
    return get_peft_model(m, cfg).train()


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("train")
    p.add_argument("--model", required=True, help="Hugging Face name or directory")
    p.add_argument("--out", required=True)
    p.add_argument("--hold-out", nargs="*", default=[], metavar="DRILL")
    p.add_argument("--epochs", type=int, default=3)
    p.add_argument("--max-tokens", type=int, default=4096)
    p.add_argument("--qlora", action="store_true", help="4-bit base (NF4)")
    p.add_argument("--rank", type=int, default=16)
    p.add_argument("--lr", type=float, default=1e-4)
    p.add_argument("--accumulate", type=int, default=8, help="examples per optimizer step")
    p.add_argument("--seed", type=int, default=0)
    args = p.parse_args()

    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    log = (out / "log.jsonl").open("a")

    def note(**rec):
        rec["time"] = time.strftime("%Y-%m-%dT%H:%M:%S")
        print(json.dumps(rec), flush=True)
        log.write(json.dumps(rec) + "\n")
        log.flush()

    tok = AutoTokenizer.from_pretrained(args.model)
    recs = [json.loads(l) for l in open(args.train)]
    train = [encode(tok, r, args.max_tokens) for r in recs if r["drill"] not in args.hold_out]
    val = [encode(tok, r, args.max_tokens) for r in recs if r["drill"] in args.hold_out]
    note(event="data", train=len(train), val=len(val), hold_out=args.hold_out,
         train_tokens=sum(len(i) for i, _ in train), learned_tokens=sum(int(m.sum()) for _, m in train),
         **{k: v for k, v in vars(args).items() if k not in ("train", "out", "hold_out")})

    model = load(args.model, args.qlora, args.rank)
    params = [p for p in model.parameters() if p.requires_grad]
    opt = torch.optim.AdamW(params, lr=args.lr, weight_decay=0.0)
    steps = args.epochs * math.ceil(len(train) / args.accumulate)
    warmup = max(1, steps // 20)
    sched = torch.optim.lr_scheduler.LambdaLR(opt, lambda s: min((s + 1) / warmup, 0.5 * (
        1 + math.cos(math.pi * min(1.0, (s - warmup) / max(1, steps - warmup))))))

    def validate():
        model.eval()
        total = n = 0
        with torch.no_grad():
            for ids, learn in val:
                total += float(loss_sum(model, ids, learn))
                n += int(learn[1:].sum())
        model.train()
        return total / max(n, 1)

    if val:
        note(event="val", epoch=0, loss=round(validate(), 4))
    rng = random.Random(args.seed)
    step = 0
    for epoch in range(1, args.epochs + 1):
        order = list(range(len(train)))
        rng.shuffle(order)
        for b in range(0, len(order), args.accumulate):
            batch = [train[i] for i in order[b:b + args.accumulate]]
            n = sum(int(m[1:].sum()) for _, m in batch)
            t, total = time.time(), 0.0
            for ids, learn in batch:
                loss = loss_sum(model, ids, learn) / n  # every learned token weighs the same
                loss.backward()
                total += float(loss)
            torch.nn.utils.clip_grad_norm_(params, 1.0)
            opt.step()
            sched.step()
            opt.zero_grad(set_to_none=True)
            step += 1
            note(event="step", epoch=epoch, step=step, of=steps, loss=round(total, 4),
                 lr=sched.get_last_lr()[0], tokens=sum(len(i) for i, _ in batch),
                 s=round(time.time() - t, 1), peak_gib=round(torch.cuda.max_memory_allocated() / 2**30, 2))
        model.save_pretrained(out / f"epoch-{epoch}")
        if val:
            note(event="val", epoch=epoch, loss=round(validate(), 4))
    note(event="done")


if __name__ == "__main__":
    main()
