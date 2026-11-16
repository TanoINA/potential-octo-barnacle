import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { InteractiveQuestionCard, type QuestionItem } from './InteractiveQuestionCard';

describe('InteractiveQuestionCard', () => {
  const sampleQuestions: QuestionItem[] = [
    {
      question: 'Which database should we use?',
      options: ['SQLite', 'PostgreSQL', 'DuckDB'],
      is_multi_select: false,
    },
  ];

  it('renders questions and clickable option chips', () => {
    render(<InteractiveQuestionCard questions={sampleQuestions} />);
    expect(screen.getByText('Which database should we use?')).toBeTruthy();
    expect(screen.getByText('SQLite')).toBeTruthy();
    expect(screen.getByText('PostgreSQL')).toBeTruthy();
    expect(screen.getByText('DuckDB')).toBeTruthy();
  });

  it('dispatches selected single-choice option immediately upon click', () => {
    const onSelect = vi.fn();
    render(<InteractiveQuestionCard questions={sampleQuestions} onSelectOption={onSelect} />);

    fireEvent.click(screen.getByText('SQLite'));
    expect(onSelect).toHaveBeenCalledWith('SQLite');
    expect(screen.getByText('Choice submitted to agent')).toBeTruthy();
  });

  it('supports multi-select with submit button', () => {
    const onSelect = vi.fn();
    const multiQuestions: QuestionItem[] = [
      {
        question: 'Which packages to install?',
        options: ['vitest', 'eslint', 'prettier'],
        is_multi_select: true,
      },
    ];

    render(<InteractiveQuestionCard questions={multiQuestions} onSelectOption={onSelect} />);

    fireEvent.click(screen.getByText('vitest'));
    fireEvent.click(screen.getByText('prettier'));
    expect(onSelect).not.toHaveBeenCalled();

    const submitBtn = screen.getByRole('button', { name: 'Submit selection' });
    fireEvent.click(submitBtn);

    expect(onSelect).toHaveBeenCalledWith('vitest, prettier');
    expect(screen.getByText('Choice submitted to agent')).toBeTruthy();
  });

  it('renders nothing when questions array is empty', () => {
    const { container } = render(<InteractiveQuestionCard questions={[]} />);
    expect(container.firstChild).toBeNull();
  });
});
