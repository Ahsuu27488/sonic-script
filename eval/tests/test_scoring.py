from sonic_eval.scoring import canon_words, format_score, has_term, list_items, score


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


GROCERY = {"list": "dash", "items": 4, "absent": [], "digits": []}


def test_a_dash_list_with_the_right_items_passes():
    out = "Ammi ne kaha hai market se ye cheezein le aana:\n- Doodh\n- Anday\n- Bread\n- 2 kilo aloo"
    fs = format_score(out, GROCERY)
    assert fs.list_ok and (fs.checks, fs.passed, fs.rate) == (1, 1, 1.0)


def test_a_list_left_as_one_sentence_or_with_a_wrong_count_fails():
    assert not format_score("Ammi ne kaha hai doodh, anday, bread aur 2 kilo aloo le aana.", GROCERY).list_ok
    assert not format_score("Lana hai:\n- Doodh\n- Anday\n- Bread aur 2 kilo aloo", GROCERY).list_ok


def test_ordered_steps_need_numbers_in_order():
    steps = {"list": "ordered", "items": 3}
    assert format_score("Tareeqa:\n1. Paani ubaalo\n2. Patti daalo\n3. Pakao", steps).list_ok
    assert not format_score("Tareeqa:\n- Paani ubaalo\n- Patti daalo\n- Pakao", steps).list_ok
    assert not format_score("Tareeqa:\n1. Paani ubaalo\n3. Patti daalo\n2. Pakao", steps).list_ok
    assert list_items("1) a\n* b\n• c\n-not an item") == [("ordered", 1), ("dash", None), ("dash", None)]


def test_any_accepts_either_marker_but_not_a_mix():
    todo = {"list": "any", "items": 2}
    assert format_score("Kaam:\n- Bank\n- Bill", todo).list_ok
    assert format_score("Kaam:\n1. Bank\n2. Bill", todo).list_ok
    assert not format_score("Kaam:\n1. Bank\n- Bill", todo).list_ok


def test_a_sentence_must_not_become_a_list():
    plain = {"list": None}
    assert format_score("Pehle main socha ke gym jaun, lekin phir baarish ho gayi.", plain).list_ok
    assert not format_score("Mujhe chahiye:\n- Charger\n- Headphones", plain).list_ok


def test_take_backs_and_fillers_must_be_gone():
    check = {"list": None, "absent": ["paanch", "5", "nahi"], "digits": ["6"]}
    fs = format_score("Theek hai, kal 6 baje milte hain.", check)
    assert (fs.absent_ok, fs.digits_ok, fs.rate) == (True, True, 1.0)
    fs = format_score("Theek hai, kal 5 baje milte hain, nahi nahi 6 baje.", check)
    assert (fs.absent_ok, fs.checks, fs.passed) == (False, 3, 2)
    fillers = {"list": None, "absent": ["matlab", "woh na"]}
    assert not format_score("Matlab yaar, woh na, kal chhutti hai?", fillers).absent_ok
    assert format_score("Yaar, kal chhutti hai? Woh nahi aaya.", fillers).absent_ok  # "woh nahi" is not "woh na"


def test_numbers_are_matched_whole():
    check = {"list": None, "digits": ["2,50,000", "2:30"], "absent": ["5"]}
    fs = format_score("Bhai, 2,50,000 rupay bhej do 2:30 baje tak.", check)
    assert fs.digits_ok and fs.absent_ok  # the 5 inside 2,50,000 is not a stray 5
    assert not format_score("Bhai, 250000 rupay bhej do dhai baje tak.", check).digits_ok

