from sonic_eval.scoring import canon_words, has_term, score


def test_latin_output_passes_script_check():
    s = score("PR merge kar do aur deployment check kar lo.", "PR merge kardo aur deployment check karlo.",
              ["PR", "merge", "deployment", "check"])
    assert s.script_ok
    assert s.english_rate == 1.0
    assert s.accuracy == 1.0  # "kar do" and "kardo" are the same once spaces are ignored


def test_urdu_or_devanagari_script_fails():
    assert not score("PR مرج کردو", "PR merge kardo", ["PR"]).script_ok
    assert not score("PR मर्ज कर दो", "PR merge kardo", ["PR"]).script_ok


def test_empty_output_fails_script_check():
    assert not score("", "Shukriya bhai", []).script_ok


def test_spelling_variants_fold_together():
    assert canon_words("Yeh bohot acha hay") == canon_words("ye bohat achha hai")
    assert score("Mujhe samajh nahin aa raha", "Mujhe samajh nahi aa raha", []).accuracy == 1.0


def test_urdu_main_is_not_folded_into_mein():
    assert canon_words("main")[0] != canon_words("mein")[0]


def test_missing_english_word_is_counted():
    s = score("Is function ko refector karo", "Is function ko refactor karo", ["function", "refactor"])
    assert s.english_found == 1 and s.english_total == 2


def test_term_match_is_whole_word_and_plural_tolerant():
    assert has_term("tests likh do", "test")
    assert has_term("ek test likh do", "tests")
    assert not has_term("protester", "test")


def test_wrong_words_lower_accuracy():
    assert score("Aaj barish ho rahi hai", "Aaj mausam bohat acha hai", []).accuracy < 0.7


def test_silence_clip_scores_only_empty_output_as_right():
    assert score("", "", []).accuracy == 1.0
    made_up = score("Bhai, yeh jo naya deployment script hai", "", [])
    assert made_up.accuracy == 0.0
