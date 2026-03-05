package msn

import (
	"strings"
)

// News categories
const (
	CategoryEarnings        = "earnings"
	CategoryDividend        = "dividend"
	CategoryCorporateAction = "corporate_action"
	CategoryRegulation      = "regulation"
	CategoryRating          = "rating"
	CategoryExpansion       = "expansion"
	CategoryLeadership      = "leadership"
	CategoryMarket          = "market"
	CategoryGeneral         = "general"
)

// Sentiment types
const (
	SentimentPositive = "positive"
	SentimentNegative = "negative"
	SentimentNeutral  = "neutral"
)

// Category keywords (Indonesian + English)
var categoryKeywords = map[string][]string{
	CategoryEarnings: {
		"laba", "rugi", "earnings", "profit", "net income", "pendapatan",
		"revenue", "keuntungan", "kerugian", "loss", "income", "untung",
		"quarterly", "kuartalan", "annual report", "laporan tahunan",
		"eps", "earning per share",
	},
	CategoryDividend: {
		"dividen", "dividend", "pembagian", "interim", "final dividend",
		"cum date", "ex date", "payment date", "tanggal pembayaran",
		"yield", "payout",
	},
	CategoryCorporateAction: {
		"akuisisi", "merger", "acquisition", "rights issue", "stock split",
		"reverse split", "buyback", "ipo", "penawaran umum", "private placement",
		"tender offer", "spin off", "spinoff", "demerger", "konsolidasi",
		"rights", "waran", "warrant", "obligasi", "bond", "sukuk",
	},
	CategoryRegulation: {
		"ojk", "regulasi", "peraturan", "kebijakan", "regulation", "policy",
		"compliance", "kepatuhan", "lisensi", "license", "izin", "permit",
		"pemerintah", "government", "bapepam", "bei", "idx", "bursa",
	},
	CategoryRating: {
		"rating", "peringkat", "upgrade", "downgrade", "outlook",
		"stable", "positive", "negative", "credit rating", "moody",
		"fitch", "s&p", "pefindo", "target price", "rekomendasi",
		"buy", "sell", "hold", "analyst",
	},
	CategoryExpansion: {
		"ekspansi", "expansion", "investasi", "investment", "proyek baru",
		"new project", "pabrik", "factory", "plant", "cabang", "branch",
		"pembangunan", "construction", "development", "joint venture", "jv",
		"kerjasama", "partnership", "kontrak", "contract",
	},
	CategoryLeadership: {
		"direktur", "director", "komisaris", "commissioner", "ceo", "cfo",
		"president director", "management", "manajemen", "direksi",
		"rups", "agm", "annual general meeting", "pengangkatan", "appointment",
		"pengunduran", "resignation", "pergantian", "change",
	},
	CategoryMarket: {
		"ihsg", "idx", "pasar modal", "bursa", "market", "saham",
		"stock", "trading", "perdagangan", "volume", "kapitalisasi",
		"market cap", "blue chip", "lq45", "idx80", "kompas100",
	},
}

// Positive sentiment keywords
var positiveKeywords = []string{
	// Indonesian
	"naik", "untung", "tumbuh", "positif", "optimis", "meningkat",
	"surplus", "berhasil", "sukses", "cemerlang", "bagus", "baik",
	"membaik", "melonjak", "meroket", "tertinggi", "rekor",
	"peningkatan", "pertumbuhan", "keuntungan", "laba bersih",
	"ekspansi", "pemulihan", "recovery",
	// English
	"rise", "gain", "growth", "positive", "optimistic", "increase",
	"surplus", "success", "excellent", "good", "improve", "surge",
	"soar", "highest", "record", "profit", "expansion", "recovery",
	"bullish", "upgrade", "beat", "exceed", "outperform",
}

// Negative sentiment keywords
var negativeKeywords = []string{
	// Indonesian
	"turun", "rugi", "anjlok", "negatif", "pesimis", "menurun",
	"defisit", "gagal", "buruk", "memburuk", "jatuh", "tertekan",
	"terendah", "penurunan", "kerugian", "merosot", "melemah",
	"default", "bangkrut", "pailit", "koreksi", "tekanan",
	// English
	"fall", "loss", "plunge", "negative", "pessimistic", "decrease",
	"deficit", "fail", "bad", "worsen", "drop", "pressure",
	"lowest", "decline", "weak", "default", "bankrupt", "correction",
	"bearish", "downgrade", "miss", "underperform", "concern", "risk",
}

// Critical news keywords (alerts)
var criticalKeywords = []string{
	// Indonesian
	"suspend", "suspensi", "fraud", "penipuan", "korupsi", "corruption",
	"default", "gagal bayar", "bangkrut", "pailit", "bankruptcy",
	"delisting", "pencabutan", "investigasi", "investigation",
	"skandal", "scandal", "illegal", "ilegal", "pelanggaran", "violation",
	"tuntutan", "lawsuit", "gugatan", "denda", "fine", "sanksi", "sanction",
	"pkpu", "penundaan", "moratorium", "restrukturisasi utang",
	// English
	"suspend", "fraud", "corruption", "default", "bankrupt", "bankruptcy",
	"delisting", "investigation", "scandal", "illegal", "violation",
	"lawsuit", "fine", "sanction", "debt restructuring", "warning",
	"material adverse", "going concern", "audit opinion", "disclaimer",
}

// categorizeNews determines the category of a news article
func categorizeNews(title, abstract string) string {
	text := strings.ToLower(title + " " + abstract)

	// Check each category
	maxScore := 0
	bestCategory := CategoryGeneral

	for category, keywords := range categoryKeywords {
		score := 0
		for _, keyword := range keywords {
			if strings.Contains(text, keyword) {
				score++
			}
		}
		if score > maxScore {
			maxScore = score
			bestCategory = category
		}
	}

	return bestCategory
}

// scoreNewsSentiment analyzes sentiment of a news article
func scoreNewsSentiment(title, abstract string) (sentiment string, score float64) {
	text := strings.ToLower(title + " " + abstract)

	positiveScore := 0
	negativeScore := 0

	for _, keyword := range positiveKeywords {
		if strings.Contains(text, keyword) {
			positiveScore++
		}
	}

	for _, keyword := range negativeKeywords {
		if strings.Contains(text, keyword) {
			negativeScore++
		}
	}

	totalScore := positiveScore + negativeScore
	if totalScore == 0 {
		return SentimentNeutral, 0.0
	}

	// Calculate score from -1 (very negative) to +1 (very positive)
	score = float64(positiveScore-negativeScore) / float64(totalScore)

	if score > 0.2 {
		sentiment = SentimentPositive
	} else if score < -0.2 {
		sentiment = SentimentNegative
	} else {
		sentiment = SentimentNeutral
	}

	return sentiment, score
}

// isNewsCritical checks if news contains critical/alert-worthy content
func isNewsCritical(title, abstract string) bool {
	text := strings.ToLower(title + " " + abstract)

	for _, keyword := range criticalKeywords {
		if strings.Contains(text, keyword) {
			return true
		}
	}

	return false
}
