#!/usr/bin/env python3
"""arXiv search and generate briefing."""

import urllib.request
import urllib.parse
import json
import subprocess
from datetime import datetime, timedelta
import xml.etree.ElementTree as ET

# User interests from profile
INTERESTS = [
    "Apple Silicon machine learning",
    "MLX framework",
    "LLM inference optimization",
    "AI agent tool use",
    "Bitcoin Lightning payment protocol",
    "Mixture of Experts architecture",
    "autonomous coding agents",
    "real estate NLP property description"
]

# Search queries for arxiv API
QUERIES = [
    "Apple Silicon machine learning",
    "MLX framework deep learning",
    "LLM quantization inference",
    "AI agent tool use planning",
    "Bitcoin Lightning protocol machine payment",
    "Mixture of Experts MoE architecture",
    "autonomous coding agent memory"
]

def search_arxiv(query, max_results=5):
    """Query arxiv API and return results."""
    base_url = "https://export.arxiv.org/api/query"
    params = {
        "search_query": f"all:{urllib.parse.quote(query)}",
        "sortBy": "submittedDate",
        "sortOrder": "descending",
        "max_results": str(max_results)
    }
    url = f"{base_url}?{urllib.parse.urlencode(params)}"
    
    try:
        with urllib.request.urlopen(url, timeout=30) as response:
            return response.read().decode('utf-8')
    except Exception as e:
        print(f"Error searching '{query}': {e}")
        return None

def parse_arxiv_xml(xml_data):
    """Parse arXiv XML and return list of papers."""
    if not xml_data:
        return []
    
    papers = []
    try:
        root = ET.fromstring(xml_data)
        namespace = {'atom': 'http://www.w3.org/2005/Atom'}
        
        entries = root.findall('atom:entry', namespace)
        for entry in entries:
            title = entry.find('atom:title', namespace)
            summary = entry.find('atom:summary', namespace)
            id_entry = entry.find('atom:id', namespace)
            published = entry.find('atom:published', namespace)
            
            if title is not None and id_entry is not None:
                # Extract paper ID from URL
                paper_id = id_entry.text.split('/')[-1]
                papers.append({
                    'title': title.text.strip() if title.text else "",
                    'abstract': summary.text.strip() if summary is not None and summary.text else "",
                    'id': paper_id,
                    'url': f"https://arxiv.org/abs/{paper_id}",
                    'published': published.text if published is not None else ""
                })
    except ET.ParseError as e:
        print(f"XML Parse error: {e}")
    
    return papers

def deduplicate_papers(papers_list):
    """Remove duplicate papers by ID."""
    seen_ids = set()
    unique_papers = []
    for paper in papers_list:
        if paper['id'] not in seen_ids:
            seen_ids.add(paper['id'])
            unique_papers.append(paper)
    return unique_papers

def get_recent_papers(papers, hours=48):
    """Filter papers from last 48 hours."""
    cutoff = datetime.now() - timedelta(hours=hours)
    recent = []
    
    for paper in papers:
        try:
            pub_date = datetime.fromisoformat(paper['published'].replace('Z', '+00:00'))
            if pub_date >= cutoff:
                recent.append(paper)
        except:
            continue
    
    return recent

def score_relevance(title, abstract, interests):
    """Simple keyword-based relevance scoring (1-10)."""
    score = 2  # Base score
    
    keywords = {
        'Apple Silicon': 2,
        'MLX': 3,
        'machine learning': 1,
        'LLM': 2,
        'inference': 2,
        'quantization': 3,
        'GPU': 1,
        'neural network': 1,
        'AI agent': 2,
        'autonomous': 2,
        'coding': 2,
        'Bitcoin': 2,
        'Lightning': 2,
        'Mixture of Experts': 3,
        'MoE': 3,
        'memory': 1,
        'tool use': 2,
        'real estate': 2,
        'property': 1,
        'NLP': 1,
        'large language model': 2,
        'local inference': 2,
        'on-device': 2,
        'vLLM': 2,
        'speculative decoding': 2,
        'KV cache': 2,
        'tokenization': 1,
        'prompt engineering': 1,
        'mechanism': 1
    }
    
    text = (title + ' ' + abstract).lower()
    
    for keyword, points in keywords.items():
        if keyword.lower() in text:
            score += points
    
    score = min(10, max(1, score))
    return score

def generate_briefing(papers, interests):
    """Generate German briefing."""
    if not papers:
        return "arXiv Briefing — " + datetime.now().strftime("%d.%m.%Y") + "\n\nHeute nichts für dich — morgen wieder."
    
    # Score and filter
    scored_papers = []
    for paper in papers:
        score = score_relevance(paper['title'], paper['abstract'], interests)
        scored_papers.append((paper, score))
    
    # Keep top 5 or papers with score >= 7
    scored_papers.sort(key=lambda x: x[1], reverse=True)
    
    filtered = []
    high_score_papers = [(p, s) for p, s in scored_papers if s >= 7]
    
    if high_score_papers:
        filtered = high_score_papers[:5]
    else:
        filtered = scored_papers[:min(5, len(scored_papers))]
    
    # Generate summary
    date_str = datetime.now().strftime("%d.%m.%Y")
    lines = [f"arXiv Briefing — {date_str}", ""]
    
    for i, (paper, score) in enumerate(filtered, 1):
        # Generate short German summary (2 sentences)
        title = paper['title'].strip()[:80] + "..." if len(paper['title']) > 80 else paper['title'].strip()
        
        # Match to project
        project = "MLX / local inference"
        if 'Bitcoin' in title or 'Lightning' in title:
            project = "L402 / machine payments"
        elif 'agent' in title.lower() or 'autonomous' in title.lower():
            project = "AI agent / memory"
        elif 'Mixture of Experts' in title or 'MoE' in title:
            project = "MoE architecture"
        elif 'quantization' in title.lower():
            project = "Quantization / GGUF"
        elif 'coding' in title.lower():
            project = "Autonomous coding"
        elif 'real estate' in title.lower():
            project = "Real estate NLP"
        
        lines.append(f"{i}. {title}")
        lines.append(f"   [{score}/10 Relevanz] {project}")
        lines.append(f"   arxiv.org/abs/{paper['id']}")
        lines.append("")
    
    return "\n".join(lines[:500])

def main():
    """Main entry point."""
    print("Searching arxiv...")
    
    all_papers = []
    for query in QUERIES:
        print(f"  Query: {query}")
        xml = search_arxiv(query, max_results=5)
        if xml:
            papers = parse_arxiv_xml(xml)
            all_papers.extend(papers)
            print(f"  Found {len(papers)} papers")
    
    # Deduplicate and filter by date
    all_papers = deduplicate_papers(all_papers)
    recent_papers = get_recent_papers(all_papers, hours=48)
    
    print(f"Total unique papers: {len(all_papers)}")
    print(f"Recent papers (48h): {len(recent_papers)}")
    
    # Generate briefing
    briefing = generate_briefing(recent_papers, INTERESTS)
    
    # Output to file for later use
    output_path = "/Users/braker/braker.tech/01_Projects/goose-webtest/logs/arxiv_briefing.md"
    with open(output_path, 'w') as f:
        f.write(briefing)
    
    print(f"\nBriefing saved to {output_path}")
    print("\n--- CONTENT ---")
    print(briefing)
    print("--- END ---")

if __name__ == "__main__":
    main()
