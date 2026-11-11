"""train.py's choice of the tokens to learn, with a tokenizer and templates made up here: torch and
the model libraries are not needed for it, and are stood in for when they are not installed."""
import sys
import types
import unittest
from unittest import mock

import support  # noqa: F401


def load_train():
    try:
        import train
        return train
    except ImportError:
        pass
    names = {"torch": [], "torch.nn": [], "torch.nn.functional": [], "torch.utils": [],
             "torch.utils.checkpoint": ["checkpoint"], "peft": ["LoraConfig", "get_peft_model"],
             "transformers": ["AutoModelForCausalLM", "AutoTokenizer", "BitsAndBytesConfig"]}
    stand_ins = {}
    for name, attrs in names.items():
        stand_ins[name] = types.ModuleType(name)
        for a in attrs:
            setattr(stand_ins[name], a, None)
    with mock.patch.dict(sys.modules, stand_ins):
        sys.modules.pop("train", None)
        import train
    sys.modules.pop("train", None)
    return train


train = load_train()


class Tokenizer:
    """One token per character; the chat template is ChatML or a bracketed one."""

    def __init__(self, style="chatml", think_in_history=False):
        self.style, self.think = style, think_in_history

    def apply_chat_template(self, messages, tools=None, tokenize=False, enable_thinking=False):
        out = []
        for m in messages:
            text = m.get("content") or ""
            for c in m.get("tool_calls") or []:
                text += f"<tool_call>{c['function']['arguments']}</tool_call>"
            if self.style == "chatml":
                think = train.EMPTY_THINK if self.think and m["role"] == "assistant" else ""
                out.append(f"<|im_start|>{m['role']}\n{think}{text}<|im_end|>\n")
            elif self.style == "brackets":
                out.append(f"[{m['role'].upper()}]{text}[/END]")
            else:
                out.append(text + "\n")
        return "".join(out)

    def __call__(self, text, return_offsets_mapping=True, add_special_tokens=False):
        return {"input_ids": [ord(c) for c in text], "offset_mapping": [(i, i + 1) for i in range(len(text))]}


REC = {"drill": "d", "seed": 100, "tools": [], "messages": [
    {"role": "system", "content": "SYS"},
    {"role": "user", "content": "TASK"},
    {"role": "assistant", "content": "", "tool_calls": [{"function": {"name": "run", "arguments": '{"command": "ls"}'}}]},
    {"role": "tool", "content": "OUT"},
    {"role": "assistant", "content": "DONE"},
]}


def learned_text(tok, rec=REC, max_tokens=10**6):
    start, end = train.markers(tok)
    ids, learn = train.learned(tok, rec, max_tokens, start, end)
    return "".join(chr(i) if keep else "\0" for i, keep in zip(ids, learn)).split("\0")


class Markers(unittest.TestCase):
    def test_chatml(self):
        self.assertEqual(train.markers(Tokenizer()), ("<|im_start|>assistant\n", "<|im_end|>"))
        self.assertEqual(train.markers(Tokenizer(think_in_history=True)), ("<|im_start|>assistant\n", "<|im_end|>"))

    def test_another_template(self):
        self.assertEqual(train.markers(Tokenizer("brackets")), ("[ASSISTANT]", "[/END]"))

    def test_a_template_without_markers_is_an_error(self):
        with self.assertRaises(ValueError):
            train.markers(Tokenizer("plain"))


class Learned(unittest.TestCase):
    def test_assistant_turns_only(self):
        parts = [p for p in learned_text(Tokenizer()) if p]
        self.assertEqual(parts, ["<tool_call>{'command': 'ls'}</tool_call><|im_end|>", "DONE<|im_end|>"])

    def test_the_empty_thinking_block_is_not_learned(self):
        parts = [p for p in learned_text(Tokenizer(think_in_history=True)) if p]
        self.assertEqual(parts, ["<tool_call>{'command': 'ls'}</tool_call><|im_end|>", "DONE<|im_end|>"])

    def test_another_template_is_learned_too(self):
        parts = [p for p in learned_text(Tokenizer("brackets")) if p]
        self.assertEqual(parts, ["<tool_call>{'command': 'ls'}</tool_call>[/END]", "DONE[/END]"])

    def test_markers_of_another_template_are_an_error_not_zero_tokens(self):
        with self.assertRaisesRegex(ValueError, "2 assistant turns"):
            train.learned(Tokenizer("brackets"), REC, 10**6, "<|im_start|>assistant\n", "<|im_end|>")

    def test_cut_at_max_tokens(self):
        ids, learn = train.learned(Tokenizer(), REC, 30, *train.markers(Tokenizer()))
        self.assertEqual((len(ids), len(learn), any(learn)), (30, 30, False))


if __name__ == "__main__":
    unittest.main()
